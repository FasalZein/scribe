# Overlap the stages within one source

One source ran its stages one after another: yt-dlp metadata, a second yt-dlp extraction for the download, model load, the whole ffmpeg decode, then the engine. The user processes a few sources at a time, so pipelining several sources gains little; the time to finish one source matters.

`--timings` on an M4 Pro (Metal, busy host, median of three or four successful runs) showed where the time went before the change:

| source | metadata | download | model | decode | engine | wall |
|---|---:|---:|---:|---:|---:|---:|
| local 27:26 mp4 | 0.03 | 0 | 0.21 | 1.04 | 12.80 | 14.36 |
| X post, 27:26 (streamed mp4) | 0.22 | 0 | 0.22 | 7.11 | 15.52 | 24.55 |
| YouTube 39:49 | 3.00 | 6.63 | 0.21 | 3.16 | 19.38 | 33.38 |

Decision, within one source and with `std::thread` and a channel only:

1. **One yt-dlp extraction.** The metadata JSON goes to the workspace, and the download reads it with `--load-info-json`. The format URLs in it stay valid for hours. YouTube download time fell from 6.63 s to 3.39 s (39:49) and from 3.92 s to 1.06 s (3:27).
2. **Model load on a loader thread**, started after the skip check, so a skipped input still loads no model. A warm load is 0.2 s; one cold load was 17 s, while macOS rebuilt its shared Metal shader cache. The overlap hides the load up to the length of the download and decode.
3. **Transcribe while decoding.** A decoder thread streams ffmpeg samples to the main thread, which runs an engine batch of 16 chunks as soon as 16 chunks are final. `audio::Chunker` decides a cut from the audio up to the chunk's hard end only, so it gives the same cuts as the whole-buffer chunking (a unit test compares both against the old implementation). Batches have the same composition as before, so the transcript is unchanged: 4,890 words and WER 3.61 % on the 27:26 talk, as in ADR 0003.

The channel is unbounded: a CPU batch can take a minute, and a bounded channel would stall ffmpeg's network read past its 30 s timeout. It holds at most the decoded audio, which scribe keeps anyway. The truncation check, atomic publish and source IDs are unchanged: the check runs on the whole audio before the last batches and before any output.

## Results

Median of the successful runs (three per source; four before-runs for YouTube, where yt-dlp failed some runs with HTTP 403), same host and flags. The local before-median includes one cold model load; without it the local gain is within noise:

| source | before | after |
|---|---:|---:|
| local 27:26 mp4 | 14.36 s | 13.67 s |
| X post, 27:26 | 24.55 s | 16.83 s |
| YouTube 39:49 | 33.38 s | 27.31 s |
| YouTube 3:27 | 9.29 s | 6.54 s |

Peak RSS on the 27:26 talk is 1.42 GB (slice A: 1.42 GB). The 39:49 talk peaks at 1.60 GB against 1.47 GB, because samples wait in the channel while a batch runs.

## Not done

- **Stream yt-dlp output into ffmpeg.** It would hide the remaining YouTube download (about 3 s on 39:49). yt-dlp can select a non-fragmented MP4 whose index sits at the end, which ffmpeg cannot read from a pipe, and `--keep-media` would need a tee. Revisit when the download is a large share of the wall time.
- **Lower-bitrate yt-dlp audio.** After change 1, the 39:49 download is 3.4 s; a smaller format saves part of that and needs a WER comparison first.
- **Engine threads.** transcribe-cpp exposes `SessionOptions::n_threads` (0 = library default) and `kv_type`; it has no warm-up or decoder-speed option. Metal runs the encoder on the GPU, so threads matter mainly for the CPU backend on Linux, which was not measured here.

## Follow-up: bounded audio memory and earlier loading (#9)

The original pipeline kept all consumed PCM in a growing buffer. Queued blocks also occupied memory while an engine batch ran. A slow CPU batch could leave hours of samples waiting in memory. Replace that handoff with a disk spool in the disk-backed media workspace, outside the system temporary directory. The decoder writes each block to disk and publishes only a sample count. The reader holds at most one engine batch plus chunking lookahead. After each batch, it discards consumed PCM and shifts the remaining chunk ranges. An absolute sample offset preserves word timestamps. The completeness check uses the decoder's total sample count, not the retained PCM length.

The decoder does not wait for the engine. Its short progress lock protects sample counts only; disk reads, disk writes and engine calls hold no lock. Thus a CPU batch that takes longer than ffmpeg's 30 s network timeout cannot stop the decoder from draining ffmpeg. Disk stalls can still delay decoding. Disk capacity must hold the whole decoded source at 64 kB/s. A disk-write failure fails the source before publication. `--keep-media` copies the completed spool to the publication stage's `audio.f32le` without loading it into memory. Normal success and failure remove the spool. A killed process can leave a hidden `.audio-*.f32le` file in its resumable media workspace. The workspace's stale sweep removes it with other expired media.

Start the model loader before the first metadata fetch and folder lookup. Join it only when transcription needs the engine. The loader uses an ordinary thread handle, which detaches when dropped. A skipped source therefore does not wait for a cold model load or download. Library subcommands do not start the loader. This changes the original skip rule: a skipped source can start a background load, but that load does not delay process exit.

Sequential `/usr/bin/time -l` runs on the M4 Pro used the 27:26 talk and two or four concatenated copies of its audio. Warm model loads took 0.25–0.30 s. No runs used `--keep-media` in this table. RSS includes engine memory and transcript text, not just PCM.

| Source length | Before peak RSS (GB) | After peak RSS (GB) | Before load averages (1/5/15 min) | After load averages (1/5/15 min) |
|---|---:|---:|---|---|
| 27:26 | 1.306 | 1.201 | 11.74 / 18.43 / 32.65 | 18.30 / 24.34 / 37.79 |
| 54:53 | 1.601 | 1.262 | 10.83 / 17.90 / 32.21 | 15.52 / 23.33 / 37.11 |
| 109:46 | 1.995 | 1.279 | 9.09 / 16.57 / 31.07 | 11.41 / 20.73 / 35.13 |

The after-RSS approaches a plateau instead of retaining source-length PCM. Host load and engine allocations still affect RSS, so these runs do not establish a constant byte limit. `segments.jsonl` was byte-identical before and after at all three lengths. A separate `--keep-media` run produced byte-identical audio. Tests also drain one hour of synthetic PCM and a real ffmpeg decode before the consumer reads anything.
