# Roadmap

What's next, in order, and what "done" means for each. Finished work moves to
[changes.md](changes.md) with its numbers. One rule throughout: **one app, one look**. The
download, the hosted site and the phone are the same RIPTV; only the edges differ.

## 1. Finish what's open (now)

- **Release 0.2.9:** episode details and cast photos, the browsing polish, offline and
  no-sound channel fixes (see "Unreleased" in changes.md).
- **Slow providers (#1):** trace the slow channels on a real account with `--logs`; change
  nothing until the numbers say where the time goes.
- **UI review on a real account:** you go through the app on your provider and list anything
  that still looks rough. Polish so far was checked on the mock provider and in screenshots.
- Done when: 0.2.9 is out, #1 has measurements, and the review list is empty.

## 2. Hosted RIPTV (a website with sign-in, on a VPS with Supabase)

**Not a second app.** The same UI and the same server binary, started in a hosted mode. The
differences are all at the edges:

| | Download (today) | Hosted |
|---|---|---|
| Who can open it | this computer only | anyone with an account |
| Sign-in | none | Supabase Auth (email, magic link, or Google) |
| Provider logins | this browser's storage | decision below |
| Watch progress, My List | this browser | synced through Supabase, per user |
| HTTPS | not needed | required (Caddy in front, automatic certificates) |
| Quit, `--logs` endpoints | on | off |
| Per-user limits | none needed | streams at once, conversions at once |

How it fits: Supabase handles accounts and stores per-user data (Postgres with row-level
security, so each user can only read their own rows). The RIPTV server checks each request's
Supabase sign-in token itself (no round trip per request) and keeps doing what it does now:
serve the app, relay the provider, convert what browsers can't play.

**Decide before building:**

1. **Who signs up?** Invite-only (you add people) or open sign-up. Invite-only is strongly
   recommended: see the risks below.
2. **Where do provider logins live?** On each device (simplest; nothing sensitive on the
   server, but you sign in again on every device), or on the server, encrypted, so they
   follow you (convenient; then the server holds everyone's provider passwords and must be
   treated that way).
3. **The VPS:** its bandwidth allowance, CPU, and whether it has a GPU.

**Risks to check first, before any code:**

- **Bandwidth:** every stream passes through the VPS. A 1080p channel is about 5 Mbit/s, so 10
  people watching is about 50 Mbit/s, sustained. Copying (no conversion) is cheap on CPU;
  converting is not, and most VPSs have no GPU.
- **Providers and data-centre IPs:** many IPTV providers block or limit server addresses,
  and every user's streams would come from the VPS's one address. Try your buddy's
  provider from the VPS with `curl` before anything else.
- **Legal:** whoever hosts it relays the streams. Keep it to people who bring their own
  subscriptions, invite-only, and make sure your buddy is comfortable with that.
- **Security:** today the server trusts that only its own computer can reach it. Hosted, every
  route needs the sign-in check, rate limits and a review of the relay before it goes live.

**Steps:** (a) the VPS checks above; (b) hosted mode behind a setting, with the sign-in check
and limits; (c) a sign-in screen in front of "Who's watching?"; (d) per-user sync of progress
and My List; (e) a Docker image and a one-page setup guide for the VPS; (f) security review.
Done when your buddy can deploy it from the guide and two accounts can't see each other's data.

## 3. Phone (the hosted site on a phone)

Same design, tuned for touch. The app already switches to a phone layout (the tab bar) and is
installable (it has a manifest and service worker); hosting gives it the HTTPS that iPhone needs
to install it to the Home Screen.

- Touch: bigger tap targets, swipe between seasons and channels, no hover-only features.
- iPhone specifics: the notch and home bar (safe areas), Safari's own fullscreen video,
  playback continuing in Picture in Picture.
- Checked the same way as desktop: Playwright on phone-sized screens, plus a real iPhone and
  Android phone.
- Done when the phone review list is empty on both.

## 4. Later

- **The experimental player stuck after resume (#6).**
- **Long-watch test (#2):** an hour or more on one channel with `--logs`, buffer and memory flat.
- **TMDB, optional:** with a free key, cast photos and episode details for the shows TVmaze
  doesn't have (e.g. "Martha Stewart Living").
- **rstreamkit:** report the Matroska interlace flag bug (RIPTV works around it).
