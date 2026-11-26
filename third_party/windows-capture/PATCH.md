# Local patch of windows-capture 2.0.1

Upstream: https://crates.io/crates/windows-capture 2.0.1 (MIT, see `LICENCE`). The crate is vendored
and selected through `[patch.crates-io]` in the workspace `Cargo.toml`.

## Change (one method, `src/encoder.rs`)

`VideoEncoder::send_frame_at(&mut self, frame: &Frame, timestamp_100ns: i64)` is `send_frame` with a
caller-chosen timestamp: the same code path (same surface handling, same channel, same error check),
but the frame is stamped with `timestamp_100ns` instead of `frame.timestamp() - first_timestamp`.

Why: `send_frame` stamps frames with their capture time. Media Foundation expects a constant frame
rate and re-times the stream when frames arrive slower than declared, which shifts the clips of R6
Coach (see `docs/v7-spec.md`, "Clip drift"). The recorder rate-limits the frames itself and stamps
the n-th sent frame at `n / fps`, so the encoder always sees a perfect constant-rate stream.

Nothing else is changed. To update the crate, copy the new release here and re-add the method.
