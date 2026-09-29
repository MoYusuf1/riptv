//! Runs the real pipeline on a real HLS segment (Big Buck Bunny, first ~200 KB of a Mux test
//! stream segment: H.264 High 848x480 + AAC-LC 44.1 kHz).

use std::process::Command;

use player::{Transmuxer, ts};

const SEGMENT: &[u8] = include_bytes!("fixtures/bbb_480p.ts");

fn tfdt(fragment: &[u8]) -> u64 {
    // The first traf in our fragments is the video track.
    let p = fragment
        .windows(4)
        .position(|w| w == b"tfdt")
        .expect("tfdt box");
    u64::from_be_bytes(fragment[p + 8..p + 16].try_into().unwrap())
}

#[test]
fn demuxes_video_and_audio_from_a_real_segment() {
    let d = ts::demux(SEGMENT).unwrap();
    assert!(
        d.video.len() > 100 && d.audio.len() > 100,
        "{} video / {} audio",
        d.video.len(),
        d.audio.len()
    );
    assert!(d.video[0].key, "segment must start on a keyframe");
    assert!(d.sps.is_some() && d.pps.is_some());
    assert_eq!(
        d.aac,
        Some(ts::AacConfig {
            object_type: 2,
            freq_index: 4,
            channels: 2
        })
    );
    // The source timestamps start ~10 s in; the muxer has to normalise that.
    assert!(
        d.video[0].dts > 800_000,
        "expected an offset start, got {}",
        d.video[0].dts
    );
    // Decode order: DTS never goes backwards, and B-frame PTS may lead DTS but never trail it.
    assert!(d.video.windows(2).all(|w| w[0].dts <= w[1].dts));
    assert!(d.video.iter().all(|s| s.pts >= s.dts));
}

#[test]
fn first_fragment_starts_at_zero_with_the_right_codecs() {
    let out = Transmuxer::default().push(SEGMENT).unwrap();
    let init = out.init.expect("first segment carries the init segment");
    assert_eq!(init.mime, "video/mp4; codecs=\"avc1.64001f,mp4a.40.2\"");
    assert_eq!(tfdt(&out.fragment), 0);
    assert!(out.skipped_audio.is_none());
}

#[test]
fn a_timestamp_jump_is_glued_onto_the_end_of_the_previous_fragment() {
    let d = ts::demux(SEGMENT).unwrap();
    let span = d.video.last().unwrap().dts - d.video[0].dts;
    assert!(
        span > 180_000,
        "fixture must be longer than the 2 s jump threshold"
    );

    // Pushing the same segment twice looks like a stream that restarted its clock.
    let mut t = Transmuxer::default();
    let first = t.push(SEGMENT).unwrap();
    let second = t.push(SEGMENT).unwrap();
    assert!(second.init.is_none());
    let start2 = tfdt(&second.fragment);
    assert!(
        start2.abs_diff(span) < 3 * 3003,
        "second fragment starts at {start2}, first spans {span}"
    );
    assert!(first.fragment.len() > 10_000 && second.fragment.len() > 10_000);
}

/// Independent check: hand the muxed bytes to ffmpeg, which must decode every frame without complaint.
#[test]
fn ffmpeg_decodes_the_output_without_errors() {
    if Command::new("ffmpeg").arg("-version").output().is_err() {
        eprintln!("ffmpeg not installed; skipping");
        return;
    }
    let d = ts::demux(SEGMENT).unwrap();
    let mut t = Transmuxer::default();
    let a = t.push(SEGMENT).unwrap();
    let b = t.push(SEGMENT).unwrap();
    let file = std::env::temp_dir().join(format!("player-test-{}.mp4", std::process::id()));
    std::fs::write(
        &file,
        [a.init.unwrap().bytes, a.fragment, b.fragment].concat(),
    )
    .unwrap();

    let decode = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(&file)
        .args(["-f", "null", "-"])
        .output()
        .unwrap();
    let complaints = String::from_utf8_lossy(&decode.stderr).into_owned();
    assert!(
        decode.status.success() && complaints.trim().is_empty(),
        "ffmpeg said: {complaints}"
    );

    let probe = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-count_frames",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=codec_name,width,height,nb_read_frames",
            "-of",
            "csv=p=0",
        ])
        .arg(&file)
        .output()
        .unwrap();
    let line = String::from_utf8_lossy(&probe.stdout).trim().to_owned();
    assert_eq!(
        line,
        format!("h264,848,480,{}", d.video.len() * 2),
        "ffprobe: {line}"
    );
    std::fs::remove_file(file).ok();
}
