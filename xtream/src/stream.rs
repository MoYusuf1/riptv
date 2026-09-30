//! Reads a provider's list (a JSON array of records) as it arrives, a record at a time.
//!
//! A big provider's list is megabytes, and the browser never gives memory back: holding the whole
//! response while parsing it doubles the peak, and waiting for all of it before starting wastes
//! the time the rest is still on its way. Here the working set is one record, and parsing overlaps
//! the download. A list that isn't an array (`{"0": {...}}`, `false`) is gathered and handed to the
//! whole-body parser, which knows those shapes.

use serde::de::DeserializeOwned;

use crate::{Error, Result, vec_from_slice};

/// No single record is anywhere near this: one that is, isn't a list of records.
const MAX_RECORD: usize = 1 << 20;
/// The most of a list that is read (a provider with more than this has bigger problems).
const MAX_LIST: usize = 256 << 20;

/// Finds the records of a JSON array in chunks of any size, without looking at what is in them.
#[derive(Default)]
struct Splitter {
    opened: bool,
    done: bool,
    depth: u32,
    in_string: bool,
    escaped: bool,
    in_record: bool,
    /// The part of a record that began in an earlier chunk.
    carry: Vec<u8>,
}

impl Splitter {
    /// Calls `emit` with each record that ends in `chunk`, as a slice of `chunk` when the whole
    /// record is in it, or of `carry` when it was split across chunks. `Err` if it isn't an array.
    fn feed(&mut self, chunk: &[u8], emit: &mut dyn FnMut(&[u8])) -> Result<()> {
        let mut start = if self.in_record { Some(0) } else { None };
        let mut finish = |this: &mut Splitter, start: &mut Option<usize>, end: usize| {
            let from = start.take().unwrap_or(0);
            if this.carry.is_empty() {
                emit(&chunk[from..end]);
            } else {
                this.carry.extend_from_slice(&chunk[from..end]);
                emit(&this.carry);
                this.carry.clear();
            }
            this.in_record = false;
        };
        for (i, &b) in chunk.iter().enumerate() {
            if self.done {
                break;
            }
            if !self.opened {
                match b {
                    b'[' => self.opened = true,
                    b if b.is_ascii_whitespace() => {}
                    _ => return Err(not_a_list()),
                }
                continue;
            }
            if self.in_string {
                match (self.escaped, b) {
                    (true, _) => self.escaped = false,
                    (false, b'\\') => self.escaped = true,
                    (false, b'"') => self.in_string = false,
                    _ => {}
                }
                continue;
            }
            let begin = |this: &mut Splitter, start: &mut Option<usize>| {
                if !this.in_record {
                    this.in_record = true;
                    *start = Some(i);
                }
            };
            match b {
                b'"' => {
                    begin(self, &mut start);
                    self.in_string = true;
                }
                b'{' | b'[' => {
                    begin(self, &mut start);
                    self.depth += 1;
                }
                b'}' | b']' if self.depth > 0 => {
                    self.depth -= 1;
                    if self.depth == 0 {
                        finish(self, &mut start, i + 1);
                    }
                }
                // The end of the list, or of a record that is a bare value (a number, `null`).
                b']' => {
                    if self.in_record {
                        finish(self, &mut start, i);
                    }
                    self.done = true;
                }
                b',' if self.depth == 0 => {
                    if self.in_record {
                        finish(self, &mut start, i);
                    }
                }
                // A comma inside a record is part of it.
                b',' => {}
                b if b.is_ascii_whitespace() => {}
                _ => begin(self, &mut start),
            }
        }
        if self.in_record {
            self.carry.extend_from_slice(&chunk[start.unwrap_or(0)..]);
            if self.carry.len() > MAX_RECORD {
                return Err(Error::Json(serde::de::Error::custom(
                    "a record is too large",
                )));
            }
        }
        Ok(())
    }
}

fn not_a_list() -> Error {
    Error::Json(serde::de::Error::custom("expected a list"))
}

/// What a list being read so far has turned into.
enum Shape<T> {
    /// Nothing but whitespace yet: the first real byte says which.
    Unknown,
    Array {
        splitter: Splitter,
        items: Vec<T>,
        first_error: Option<serde_json::Error>,
    },
    /// Something else (an object keyed by position, `false`): kept whole.
    Whole(Vec<u8>),
}

pub(crate) struct ListReader<T> {
    shape: Shape<T>,
    bytes: usize,
}

impl<T: DeserializeOwned> ListReader<T> {
    pub fn new() -> Self {
        ListReader {
            shape: Shape::Unknown,
            bytes: 0,
        }
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Result<()> {
        self.bytes += chunk.len();
        if self.bytes > MAX_LIST {
            return Err(Error::Json(serde::de::Error::custom(
                "the list is too large",
            )));
        }
        if matches!(self.shape, Shape::Unknown) {
            let Some(first) = chunk.iter().find(|b| !b.is_ascii_whitespace()) else {
                return Ok(());
            };
            self.shape = if *first == b'[' {
                Shape::Array {
                    splitter: Splitter::default(),
                    items: vec![],
                    first_error: None,
                }
            } else {
                Shape::Whole(vec![])
            };
        }
        match &mut self.shape {
            Shape::Array {
                splitter,
                items,
                first_error,
            } => splitter.feed(
                chunk,
                &mut |record: &[u8]| match serde_json::from_slice::<T>(record) {
                    Ok(item) => items.push(item),
                    // One malformed record can't sink the other 20,000.
                    Err(e) => {
                        first_error.get_or_insert(e);
                    }
                },
            ),
            Shape::Whole(all) => {
                all.extend_from_slice(chunk);
                Ok(())
            }
            Shape::Unknown => Ok(()),
        }
    }

    pub fn finish(self) -> Result<Vec<T>> {
        match self.shape {
            // Nothing at all came: not JSON.
            Shape::Unknown => vec_from_slice(b""),
            Shape::Whole(all) => vec_from_slice(&all),
            Shape::Array {
                splitter,
                items,
                first_error,
            } => {
                if !splitter.done {
                    return Err(Error::Json(serde::de::Error::custom(
                        "the list ends unexpectedly",
                    )));
                }
                // The same policy as whole bodies: an error only if every record was bad.
                match first_error {
                    Some(e) if items.is_empty() => Err(e.into()),
                    _ => Ok(items),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Rec {
        id: u64,
        #[serde(default)]
        name: String,
    }

    /// The whole-body parser and the streaming one must agree, however the bytes are cut up.
    fn agrees(json: &str) {
        let whole = vec_from_slice::<Rec>(json.as_bytes());
        for size in [1, 2, 3, 5, 7, 16, 1000] {
            let mut reader = ListReader::<Rec>::new();
            let fed: Result<()> = json
                .as_bytes()
                .chunks(size)
                .try_for_each(|c| reader.feed(c));
            let streamed = fed.and_then(|()| reader.finish());
            match (&whole, &streamed) {
                (Ok(a), Ok(b)) => assert_eq!(a, b, "chunks of {size}: {json}"),
                (Err(_), Err(_)) => {}
                (a, b) => panic!("chunks of {size}: {a:?} vs {b:?} for {json}"),
            }
        }
    }

    #[test]
    fn streaming_reads_what_the_whole_body_parser_reads() {
        agrees(r#"[{"id":1,"name":"a"},{"id":2,"name":"b"}]"#);
        agrees("  [ {\"id\": 1 , \"name\" : \"x\"} ,\n {\"id\":2}\t]  ");
        // Brackets, commas and quotes inside strings, escapes included.
        agrees(r#"[{"id":1,"name":"a]}, \"b\" [{"},{"id":2,"name":"\\"},{"id":3,"name":"é, ]"}]"#);
        // Nested values in records.
        agrees(r#"[{"id":1,"extra":{"a":[1,2,{"b":[]}],"c":"}"},"name":"n"}]"#);
        agrees("[]");
        agrees("[ ]");
        // A bad record is skipped, not fatal; all bad is an error.
        agrees(r#"[{"id":1},{"id":"x"},{"id":3}]"#);
        agrees(r#"[1,"two",null,{"id":4},true]"#);
        agrees(r#"[{"name":"no id"}]"#);
        // Not arrays, or not finished, or not JSON.
        agrees(r#"{"0":{"id":1},"1":{"id":2}}"#);
        agrees("false");
        agrees("null");
        agrees(r#"[{"id":1},{"id":2}"#);
        agrees("not json at all");
        agrees("");
    }

    #[test]
    fn a_record_is_never_held_longer_than_it_is_split() {
        // A list of a thousand records, fed a few bytes at a time, never keeps more than one.
        let json = format!(
            "[{}]",
            (0..1000)
                .map(|i| format!(r#"{{"id":{i},"name":"record number {i}"}}"#))
                .collect::<Vec<_>>()
                .join(",")
        );
        let mut reader = ListReader::<Rec>::new();
        let mut most = 0;
        for chunk in json.as_bytes().chunks(13) {
            reader.feed(chunk).unwrap();
            if let Shape::Array { splitter, .. } = &reader.shape {
                most = most.max(splitter.carry.len());
            }
        }
        assert!(most < 64, "kept {most} bytes of a record at most");
        assert_eq!(reader.finish().unwrap().len(), 1000);
    }

    #[test]
    fn a_record_that_never_ends_is_refused() {
        let mut reader = ListReader::<Rec>::new();
        reader.feed(b"[{\"id\":1,\"name\":\"").unwrap();
        let chunk = vec![b'x'; 1 << 16];
        let refused = (0..32).any(|_| reader.feed(&chunk).is_err());
        assert!(refused);
    }
}
