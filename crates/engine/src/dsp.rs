//! Small DSP building blocks shared by the engine and the app.

/// In-place radix-2 FFT of `re`/`im` (length a power of two).
pub fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let angle = -std::f32::consts::TAU / len as f32;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (w_re, w_im) = ((angle * k as f32).cos(), (angle * k as f32).sin());
                let (a, b) = (start + k, start + k + len / 2);
                let t_re = re[b] * w_re - im[b] * w_im;
                let t_im = re[b] * w_im + im[b] * w_re;
                re[b] = re[a] - t_re;
                im[b] = im[a] - t_im;
                re[a] += t_re;
                im[a] += t_im;
            }
        }
        len <<= 1;
    }
}

/// In-place inverse FFT (scaled by 1/n), via the conjugate trick.
pub fn ifft(re: &mut [f32], im: &mut [f32]) {
    for v in im.iter_mut() {
        *v = -*v;
    }
    fft(re, im);
    let scale = 1.0 / re.len() as f32;
    for (r, i) in re.iter_mut().zip(im.iter_mut()) {
        *r *= scale;
        *i = -*i * scale;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverse_fft_restores_the_signal() {
        let signal: Vec<f32> = (0..64).map(|i| ((i * 7) % 13) as f32 - 6.0).collect();
        let (mut re, mut im) = (signal.clone(), vec![0.0; 64]);
        fft(&mut re, &mut im);
        ifft(&mut re, &mut im);
        for (a, b) in re.iter().zip(&signal) {
            assert!((a - b).abs() < 1e-4);
        }
    }
}
