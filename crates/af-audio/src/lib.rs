//! Audio playback engine — streaming download, Symphonia decoding, cpal output.

use std::io;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use futures::StreamExt;
use tokio::sync::mpsc;

use af_core::events::{AudioEvent, PlaybackCommand};
use af_core::types::TrackId;

// ── Shared state between audio control loop and cpal callback ─────────────────

struct SharedState {
    samples: Vec<f32>,   // interleaved, at device sample rate / channel count
    channels: usize,
    sample_rate: u32,
    pos: usize,          // current read position in `samples`
    playing: bool,
    volume: f32,         // 0.0 .. 1.0
    done: bool,          // set by callback when track reaches end
    epoch: u64,          // bumped on every Stop/Play; cancels stale tasks
}

impl SharedState {
    fn new(sample_rate: u32, channels: usize) -> Self {
        Self {
            samples: Vec::new(),
            channels,
            sample_rate,
            pos: 0,
            playing: false,
            volume: 0.8,
            done: false,
            epoch: 0,
        }
    }

    fn position(&self) -> Duration {
        if self.sample_rate == 0 || self.channels == 0 { return Duration::ZERO; }
        Duration::from_secs_f64(
            self.pos as f64 / (self.sample_rate as f64 * self.channels as f64),
        )
    }

    fn total_duration(&self) -> Duration {
        if self.sample_rate == 0 || self.channels == 0 { return Duration::ZERO; }
        Duration::from_secs_f64(
            self.samples.len() as f64 / (self.sample_rate as f64 * self.channels as f64),
        )
    }
}

// ── Streaming download buffer ─────────────────────────────────────────────────

struct DownloadBuf {
    data:  Vec<u8>,
    done:  bool,
    error: Option<String>,
}

/// A `MediaSource` backed by a concurrently-growing download buffer.
/// Blocks the calling (blocking) thread when it needs bytes that haven't
/// arrived yet. Cancels via the epoch field in SharedState.
struct GrowingReader {
    buf:      Arc<Mutex<DownloadBuf>>,
    shared:   Arc<Mutex<SharedState>>,
    pos:      usize,
    my_epoch: u64,
}

impl io::Read for GrowingReader {
    fn read(&mut self, dst: &mut [u8]) -> io::Result<usize> {
        if dst.is_empty() { return Ok(0); }
        loop {
            if self.shared.lock().map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Lock poisoned: {e}")))?.epoch != self.my_epoch {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
            {
                let dl = self.buf.lock().map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Lock poisoned: {e}")))?;
                if let Some(ref e) = dl.error {
                    return Err(io::Error::new(io::ErrorKind::Other, e.clone()));
                }
                let avail = dl.data.len().saturating_sub(self.pos);
                if avail > 0 {
                    let n = dst.len().min(avail);
                    dst[..n].copy_from_slice(&dl.data[self.pos..self.pos + n]);
                    self.pos += n;
                    return Ok(n);
                }
                if dl.done { return Ok(0); }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl io::Seek for GrowingReader {
    fn seek(&mut self, from: io::SeekFrom) -> io::Result<u64> {
        let new_pos: usize = match from {
            io::SeekFrom::Start(n) => n as usize,
            io::SeekFrom::Current(n) => (self.pos as i64 + n).max(0) as usize,
            io::SeekFrom::End(n) => {
                // Must wait for full download to know total size.
                // This only happens for container formats (M4A) whose index is
                // at the end; for FLAC / OGG / MP3 this branch is never taken.
                loop {
                    if self.shared.lock().map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Lock poisoned: {e}")))?.epoch != self.my_epoch {
                        return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
                    }
                    let dl = self.buf.lock().map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Lock poisoned: {e}")))?;
                    if dl.done {
                        let end = dl.data.len() as i64;
                        let p   = (end + n).max(0) as usize;
                        drop(dl);
                        self.pos = p;
                        return Ok(p as u64);
                    }
                    drop(dl);
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        };
        // For a forward seek: wait until enough bytes are available.
        loop {
            if self.shared.lock().map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Lock poisoned: {e}")))?.epoch != self.my_epoch {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
            let dl = self.buf.lock().map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Lock poisoned: {e}")))?;
            if new_pos <= dl.data.len() || dl.done { break; }
            drop(dl);
            std::thread::sleep(Duration::from_millis(10));
        }
        self.pos = new_pos;
        Ok(new_pos as u64)
    }
}

impl symphonia::core::io::MediaSource for GrowingReader {
    fn is_seekable(&self) -> bool { true }
    fn byte_len(&self) -> Option<u64> {
        let dl = self.buf.lock().unwrap_or_else(|e| e.into_inner());
        if dl.done { Some(dl.data.len() as u64) } else { None }
    }
}

// ── Helpers for graceful mutex locking ────────────────────────────────────────

fn lock_state(shared: &Arc<Mutex<SharedState>>) -> MutexGuard<'_, SharedState> {
    shared.lock().unwrap_or_else(|e| {
        tracing::error!("SharedState mutex poisoned: {e}");
        e.into_inner()
    })
}

fn lock_dl(dl: &Arc<Mutex<DownloadBuf>>) -> MutexGuard<'_, DownloadBuf> {
    dl.lock().unwrap_or_else(|e| {
        tracing::error!("DownloadBuf mutex poisoned: {e}");
        e.into_inner()
    })
}

// ── Public entry point ────────────────────────────────────────────────────────

pub fn spawn(cmd_rx: mpsc::Receiver<PlaybackCommand>, event_tx: mpsc::Sender<AudioEvent>) -> anyhow::Result<()> {
    std::thread::Builder::new()
        .name("af-audio".into())
        .spawn(move || {
            match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt.block_on(audio_main(cmd_rx, event_tx)),
                Err(e) => {
                    let _ = event_tx.blocking_send(AudioEvent::Error(format!("Audio runtime error: {e}")));
                }
            }
        })
        .map_err(|e| anyhow::anyhow!("Failed to spawn audio thread: {e}"))?;
    Ok(())
}

// ── Audio main loop ───────────────────────────────────────────────────────────

async fn audio_main(
    mut cmd_rx: mpsc::Receiver<PlaybackCommand>,
    event_tx:   mpsc::Sender<AudioEvent>,
) {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    let host = cpal::default_host();

    let device = match host.default_output_device() {
        Some(d) => d,
        None => {
            let _ = event_tx.send(AudioEvent::Error("No audio output device found".into())).await;
            return;
        }
    };

    let supported = match device.default_output_config() {
        Ok(c) => c,
        Err(e) => {
            let _ = event_tx.send(AudioEvent::Error(format!("Audio config error: {e}"))).await;
            return;
        }
    };

    let device_rate = supported.sample_rate().0;
    let device_ch   = supported.channels() as usize;

    let shared = Arc::new(Mutex::new(SharedState::new(device_rate, device_ch)));

    let stream = match build_stream(&device, &supported, Arc::clone(&shared)) {
        Ok(s) => s,
        Err(e) => {
            let _ = event_tx.send(AudioEvent::Error(format!("Stream build error: {e}"))).await;
            return;
        }
    };

    if let Err(e) = stream.play() {
        let _ = event_tx.send(AudioEvent::Error(format!("Stream play error: {e}"))).await;
        return;
    }

    tracing::info!("audio engine ready — {device_rate} Hz, {device_ch}ch");

    // Position ticker: emits PositionChanged every 500 ms; detects natural track end.
    let shared_tick = Arc::clone(&shared);
    let tx_tick     = event_tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(500));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let (position, duration, playing, done) = {
                let mut st = lock_state(&shared_tick);
                let pos  = st.position();
                let dur  = st.total_duration();
                let play = st.playing;
                let done = if st.done { st.done = false; true } else { false };
                (pos, dur, play, done)
            };
            if playing {
                let _ = tx_tick.send(AudioEvent::PositionChanged { position, duration }).await;
            }
            if done {
                let _ = tx_tick.send(AudioEvent::TrackChanged(None)).await;
            }
        }
    });

    // Command loop — never blocks: Play spawns async+blocking tasks and returns immediately.
    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            PlaybackCommand::Play { track_id, stream_url } => {
                // Bump epoch — all in-flight tasks for the old epoch will see the
                // mismatch and stop without writing to SharedState.
                let my_epoch = {
                    let mut st = lock_state(&shared);
                    st.epoch = st.epoch.wrapping_add(1);
                    st.playing = false;
                    st.samples.clear();
                    st.pos  = 0;
                    st.done = false;
                    st.epoch
                };

                let download_buf = Arc::new(Mutex::new(DownloadBuf {
                    data: Vec::new(), done: false, error: None,
                }));

                // Task A: stream HTTP body into DownloadBuf.
                let dl_buf    = Arc::clone(&download_buf);
                let shared_dl = Arc::clone(&shared);
                tokio::spawn(async move {
                    let result: anyhow::Result<()> = async {
                        let client = reqwest::Client::builder()
                            .timeout(Duration::from_secs(300))
                            .build()?;
                        let resp = client
                            .get(&stream_url)
                            .send()
                            .await?
                            .error_for_status()?;
                        let mut stream = resp.bytes_stream();
                        while let Some(chunk) = stream.next().await {
                            if lock_state(&shared_dl).epoch != my_epoch { break; }
                            lock_dl(&dl_buf).data.extend_from_slice(&chunk?);
                        }
                        Ok(())
                    }.await;
                    let mut dl = lock_dl(&dl_buf);
                    if let Err(e) = result {
                        dl.error = Some(e.to_string());
                    }
                    dl.done = true;
                });

                // Task B: decode progressively in a blocking thread.
                let dec_buf    = Arc::clone(&download_buf);
                let dec_shared = Arc::clone(&shared);
                let dec_tx     = event_tx.clone();
                let sr = device_rate;
                let ch = device_ch;
                tokio::task::spawn_blocking(move || {
                    decode_progressive(dec_buf, dec_shared, dec_tx, sr, ch, my_epoch, track_id);
                });
            }

            PlaybackCommand::Stop => {
                {
                    let mut st = lock_state(&shared);
                    st.epoch = st.epoch.wrapping_add(1);
                    st.playing = false;
                    st.samples.clear();
                    st.pos  = 0;
                    st.done = false;
                }
                let _ = event_tx.send(AudioEvent::StateChanged { is_playing: false }).await;
            }

            PlaybackCommand::Pause => {
                lock_state(&shared).playing = false;
                let _ = event_tx.send(AudioEvent::StateChanged { is_playing: false }).await;
            }

            PlaybackCommand::Resume => {
                let ok = {
                    let mut st = lock_state(&shared);
                    if !st.samples.is_empty() && st.pos < st.samples.len() {
                        st.playing = true;
                        true
                    } else { false }
                };
                if ok {
                    let _ = event_tx.send(AudioEvent::StateChanged { is_playing: true }).await;
                }
            }

            PlaybackCommand::Seek(pos) => {
                let mut st = lock_state(&shared);
                if !st.samples.is_empty() {
                    let target = (pos.as_secs_f64()
                        * st.sample_rate as f64
                        * st.channels as f64) as usize;
                    st.pos = target.min(st.samples.len().saturating_sub(1));
                }
            }

            PlaybackCommand::SetVolume(v) => {
                lock_state(&shared).volume = v as f32 / 100.0;
            }

            PlaybackCommand::Next | PlaybackCommand::Previous => {
                lock_state(&shared).playing = false;
            }
        }
    }

    drop(stream);
}

// ── Progressive decoder (runs on a blocking thread) ───────────────────────────

/// Decode packets from `download_buf` as they arrive and append decoded samples
/// to `shared.samples`. Starts playback after a short prebuffer (2 s).
fn decode_progressive(
    download_buf: Arc<Mutex<DownloadBuf>>,
    shared:       Arc<Mutex<SharedState>>,
    event_tx:     mpsc::Sender<AudioEvent>,
    device_rate:  u32,
    device_ch:    usize,
    my_epoch:     u64,
    track_id:     TrackId,
) {
    use symphonia::core::{
        audio::SampleBuffer,
        codecs::DecoderOptions,
        errors::Error as SE,
        formats::FormatOptions,
        io::MediaSourceStream,
        meta::MetadataOptions,
        probe::Hint,
    };

    let reader = GrowingReader {
        buf: Arc::clone(&download_buf),
        shared: Arc::clone(&shared),
        pos: 0,
        my_epoch,
    };
    let mss = MediaSourceStream::new(Box::new(reader), Default::default());

    let probed = match symphonia::default::get_probe().format(
        &Hint::new(), mss,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    ) {
        Ok(p) => p,
        Err(e) => {
            if lock_state(&shared).epoch == my_epoch {
                let _ = event_tx.blocking_send(
                    AudioEvent::Error(format!("Format: {e}")));
            }
            return;
        }
    };

    let mut format  = probed.format;
    let track = match format.default_track() {
        Some(t) => t,
        None => {
            if lock_state(&shared).epoch == my_epoch {
                let _ = event_tx.blocking_send(
                    AudioEvent::Error("No audio track".into()));
            }
            return;
        }
    };

    let sym_id   = track.id;
    let src_rate = track.codec_params.sample_rate.unwrap_or(44100);
    let src_ch   = track.codec_params.channels.map(|c| c.count()).unwrap_or(2);

    let mut decoder = match symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
    {
        Ok(d) => d,
        Err(e) => {
            if lock_state(&shared).epoch == my_epoch {
                let _ = event_tx.blocking_send(
                    AudioEvent::Error(format!("Codec: {e}")));
            }
            return;
        }
    };

    // Start playing once 2 s of decoded audio are buffered.
    let prebuffer = (2.0 * device_rate as f64 * device_ch as f64) as usize;
    let mut started = false;

    loop {
        if lock_state(&shared).epoch != my_epoch { return; }

        let packet = match format.next_packet() {
            Ok(p)  => p,
            Err(SE::IoError(e)) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(SE::IoError(e)) if e.kind() == io::ErrorKind::Interrupted   => return,
            Err(SE::ResetRequired) => { decoder.reset(); continue; }
            Err(e) => {
                if lock_state(&shared).epoch == my_epoch {
                    let _ = event_tx.blocking_send(
                        AudioEvent::Error(format!("Packet: {e}")));
                }
                return;
            }
        };

        if packet.track_id() != sym_id { continue; }

        let buf = match decoder.decode(&packet) {
            Ok(b)  => b,
            Err(SE::IoError(_) | SE::DecodeError(_)) => continue,
            Err(e) => {
                if lock_state(&shared).epoch == my_epoch {
                    let _ = event_tx.blocking_send(
                        AudioEvent::Error(format!("Decode: {e}")));
                }
                return;
            }
        };

        let spec = *buf.spec();
        let mut sb = SampleBuffer::<f32>::new(buf.capacity() as u64, spec);
        sb.copy_interleaved_ref(buf);
        let mut new_samples = sb.samples().to_vec();

        if src_rate != device_rate || src_ch != device_ch {
            new_samples = resample(new_samples, src_ch, src_rate, device_ch, device_rate);
        }

        let total = {
            let mut st = lock_state(&shared);
            if st.epoch != my_epoch { return; }
            st.samples.extend_from_slice(&new_samples);
            st.samples.len()
        };

        if !started && total >= prebuffer {
            started = true;
            {
                let mut st = lock_state(&shared);
                if st.epoch != my_epoch { return; }
                st.playing = true;
            }
            let _ = event_tx.blocking_send(AudioEvent::TrackChanged(Some(track_id.clone())));
            let _ = event_tx.blocking_send(AudioEvent::StateChanged { is_playing: true });
        }
    }

    // All packets decoded — start playing if prebuffer was never filled
    // (e.g. very short track).
    if !started {
        let has = {
            let mut st = lock_state(&shared);
            if st.epoch != my_epoch { return; }
            if !st.samples.is_empty() { st.playing = true; true } else { false }
        };
        if has {
            let _ = event_tx.blocking_send(AudioEvent::TrackChanged(Some(track_id)));
            let _ = event_tx.blocking_send(AudioEvent::StateChanged { is_playing: true });
        }
    }
}

// ── cpal stream ───────────────────────────────────────────────────────────────

fn build_stream(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    shared: Arc<Mutex<SharedState>>,
) -> anyhow::Result<cpal::Stream> {
    use cpal::traits::DeviceTrait;
    use cpal::SampleFormat;

    let cfg = config.config();

    let stream = match config.sample_format() {
        SampleFormat::F32 => {
            let s = shared.clone();
            device.build_output_stream(
                &cfg,
                move |data: &mut [f32], _| fill_f32(data, &s),
                |e| tracing::error!("cpal error: {e}"),
                None,
            )?
        }
        SampleFormat::I16 => {
            let s = shared.clone();
            device.build_output_stream(
                &cfg,
                move |data: &mut [i16], _| {
                    let mut buf = vec![0f32; data.len()];
                    fill_f32(&mut buf, &s);
                    for (d, f) in data.iter_mut().zip(buf) {
                        *d = (f.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                    }
                },
                |e| tracing::error!("cpal error: {e}"),
                None,
            )?
        }
        SampleFormat::U16 => {
            let s = shared.clone();
            device.build_output_stream(
                &cfg,
                move |data: &mut [u16], _| {
                    let mut buf = vec![0f32; data.len()];
                    fill_f32(&mut buf, &s);
                    for (d, f) in data.iter_mut().zip(buf) {
                        *d = ((f.clamp(-1.0, 1.0) + 1.0) * 0.5 * u16::MAX as f32) as u16;
                    }
                },
                |e| tracing::error!("cpal error: {e}"),
                None,
            )?
        }
        fmt => anyhow::bail!("unsupported sample format: {fmt:?}"),
    };

    Ok(stream)
}

fn fill_f32(data: &mut [f32], shared: &Arc<Mutex<SharedState>>) {
    let mut st = lock_state(shared);
    for sample in data.iter_mut() {
        if st.playing && st.pos < st.samples.len() {
            *sample = st.samples[st.pos] * st.volume;
            st.pos += 1;
        } else {
            *sample = 0.0;
            if st.playing && !st.samples.is_empty() && !st.done {
                st.playing = false;
                st.done    = true;
            }
        }
    }
}

// ── Linear-interpolation resampler ───────────────────────────────────────────

fn resample(src: Vec<f32>, src_ch: usize, src_rate: u32, dst_ch: usize, dst_rate: u32) -> Vec<f32> {
    let src_ch     = src_ch.max(1);
    let frames     = src.len() / src_ch;
    let ratio      = src_rate as f64 / dst_rate as f64;
    let dst_frames = (frames as f64 / ratio) as usize;
    let mut dst    = Vec::with_capacity(dst_frames * dst_ch);

    for i in 0..dst_frames {
        let src_pos = i as f64 * ratio;
        let lo      = src_pos as usize;
        let frac    = (src_pos - lo as f64) as f32;
        for ch in 0..dst_ch {
            let src_c = ch.min(src_ch - 1);
            let s0 = src.get(lo * src_ch + src_c).copied().unwrap_or(0.0);
            let s1 = src.get((lo + 1) * src_ch + src_c).copied().unwrap_or(s0);
            dst.push(s0 + (s1 - s0) * frac);
        }
    }

    dst
}
