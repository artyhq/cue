use std::collections::VecDeque;

pub const MIX_RATE: u32 = 48_000;
pub const QUEUE_CAP: usize = (MIX_RATE as usize) * 2 * 2; // 2 seconds of stereo

pub fn to_stereo(data: &[f32], channels: usize) -> Vec<f32> {
    if channels == 0 {
        return Vec::new();
    }
    if channels == 2 {
        return data.to_vec();
    }
    if channels == 1 {
        let mut out = Vec::with_capacity(data.len() * 2);
        for &s in data {
            out.push(s);
            out.push(s);
        }
        return out;
    }
    let frames = data.len() / channels;
    let mut out = Vec::with_capacity(frames * 2);
    for i in 0..frames {
        let base = i * channels;
        out.push(data[base]);
        out.push(if channels > 1 {
            data[base + 1]
        } else {
            data[base]
        });
    }
    out
}

/// Linear interpolator that keeps phase across capture callbacks.
/// Independent per-chunk resampling clicks at the callback rate and sounds robotic.
pub struct StereoResampler {
    from: u32,
    to: u32,
    last_l: f32,
    last_r: f32,
    has_last: bool,
    pos: f64,
}

impl StereoResampler {
    pub fn new(from: u32, to: u32) -> Self {
        Self {
            from: from.max(1),
            to: to.max(1),
            last_l: 0.0,
            last_r: 0.0,
            has_last: false,
            pos: 0.0,
        }
    }

    pub fn convert(&mut self, data: &[f32], channels: usize) -> Vec<f32> {
        self.resample(&to_stereo(data, channels))
    }

    pub fn resample(&mut self, input: &[f32]) -> Vec<f32> {
        if self.from == self.to {
            return input.to_vec();
        }
        let in_frames = input.len() / 2;
        if in_frames == 0 {
            return Vec::new();
        }

        let offset = usize::from(self.has_last);
        let total = offset + in_frames;
        let step = self.from as f64 / self.to as f64;

        let frame = |i: usize| -> (f32, f32) {
            if self.has_last && i == 0 {
                (self.last_l, self.last_r)
            } else {
                let j = i - offset;
                (input[j * 2], input[j * 2 + 1])
            }
        };

        let mut out = Vec::new();
        while self.pos + 1.0 < total as f64 {
            let i0 = self.pos.floor() as usize;
            let t = (self.pos - i0 as f64) as f32;
            let (l0, r0) = frame(i0);
            let (l1, r1) = frame(i0 + 1);
            out.push(l0 + (l1 - l0) * t);
            out.push(r0 + (r1 - r0) * t);
            self.pos += step;
        }

        self.last_l = input[(in_frames - 1) * 2];
        self.last_r = input[(in_frames - 1) * 2 + 1];
        self.has_last = true;
        self.pos -= (total - 1) as f64;
        if self.pos < 0.0 {
            self.pos = 0.0;
        }
        out
    }
}

pub fn enqueue(queue: &mut VecDeque<f32>, samples: &[f32], cap: usize) {
    queue.extend(samples.iter().copied());
    let cap = cap.max(2) & !1;
    while queue.len() > cap {
        queue.pop_front();
        queue.pop_front();
    }
}

/// Mix only samples that every source has, so streams stay time-aligned.
pub fn mix_aligned(queues: &mut [VecDeque<f32>]) -> Vec<f32> {
    if queues.is_empty() {
        return Vec::new();
    }
    let ready = queues.iter().map(|q| q.len()).min().unwrap_or(0) / 2 * 2;
    let mut out = Vec::with_capacity(ready);
    for _ in 0..ready {
        let mut mixed = 0.0f32;
        for q in queues.iter_mut() {
            mixed += q.pop_front().unwrap_or(0.0);
        }
        out.push(mixed.clamp(-1.0, 1.0));
    }
    out
}

pub fn pad_to(queues: &mut [VecDeque<f32>], target: usize) {
    let target = target / 2 * 2;
    for q in queues {
        while q.len() < target {
            q.push_back(0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_waits_for_every_source() {
        let mut queues = vec![
            VecDeque::from(vec![0.10, 0.20, 0.30, 0.40]),
            VecDeque::new(),
        ];
        let mixed = mix_aligned(&mut queues);
        assert!(mixed.is_empty());
        assert_eq!(queues[0].len(), 4);
    }

    #[test]
    fn mix_sums_overlapping_samples() {
        let mut queues = vec![
            VecDeque::from(vec![0.10, 0.20, 0.30, 0.40]),
            VecDeque::from(vec![0.50, 0.60]),
        ];
        let mixed = mix_aligned(&mut queues);
        assert_eq!(mixed.len(), 2);
        assert!((mixed[0] - 0.60).abs() < 1e-6);
        assert!((mixed[1] - 0.80).abs() < 1e-6);
        assert_eq!(Vec::from(queues[0].clone()), vec![0.30, 0.40]);
        assert!(queues[1].is_empty());
    }

    #[test]
    fn mix_does_not_serialize_sources() {
        // Two sources each contributing one callback. Mixing after each packet
        // independently would emit A then B (robotic chopping). Aligned mix
        // emits the sum of the same-time samples.
        let mut queues = vec![VecDeque::new(), VecDeque::new()];
        enqueue(&mut queues[0], &[0.25, -0.25], QUEUE_CAP);
        enqueue(&mut queues[1], &[0.50, 0.50], QUEUE_CAP);
        let mixed = mix_aligned(&mut queues);
        assert_eq!(mixed.len(), 2);
        assert!((mixed[0] - 0.75).abs() < 1e-6);
        assert!((mixed[1] - 0.25).abs() < 1e-6);
        assert!(queues.iter().all(|q| q.is_empty()));
    }

    fn sine_stereo(frames: usize, rate: u32, hz: f32) -> Vec<f32> {
        let mut out = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let s = (2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32).sin();
            out.push(s);
            out.push(s * 0.5);
        }
        out
    }

    #[test]
    fn resampler_passthrough_when_rates_match() {
        let mut rs = StereoResampler::new(48_000, 48_000);
        let input = vec![0.1, 0.2, 0.3, 0.4];
        assert_eq!(rs.resample(&input), input);
    }

    #[test]
    fn resampler_is_continuous_across_chunks() {
        let src_rate = 44_100;
        let dst_rate = 48_000;
        let src = sine_stereo(4410, src_rate, 440.0);

        let mut all_at_once = StereoResampler::new(src_rate, dst_rate);
        let expected = all_at_once.resample(&src);

        let mut chunked = StereoResampler::new(src_rate, dst_rate);
        let mut got = Vec::new();
        for chunk in src.chunks(256) {
            // 128 stereo frames
            got.extend(chunked.resample(chunk));
        }

        assert!(!expected.is_empty());
        let n = expected.len().min(got.len());
        assert!(n > 1000);
        let mut max_err = 0.0f32;
        for i in 0..n {
            max_err = max_err.max((expected[i] - got[i]).abs());
        }
        assert!(
            max_err < 1e-4,
            "chunked resample diverged from streaming resample, max err {max_err}"
        );
    }

    #[test]
    fn resampler_maintains_rate() {
        let mut rs = StereoResampler::new(44_100, 48_000);
        let input = sine_stereo(4410, 44_100, 440.0);
        let out = rs.resample(&input);
        let out_frames = out.len() / 2;
        // 4410 input frames at 44100 -> 4800 output frames at 48000
        assert!((out_frames as i32 - 4800).abs() <= 2);
    }

    #[test]
    fn to_stereo_duplicates_mono() {
        assert_eq!(to_stereo(&[0.2, 0.4], 1), vec![0.2, 0.2, 0.4, 0.4]);
    }
}
