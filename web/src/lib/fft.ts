export interface SpectrumEntry {
  freq: string;
  period: string;
  magnitude: number;
  index: number;
}

/** In-place Cooley-Tukey radix-2 DIT FFT */
export function fft(re: number[], im: number[]): void {
  const N = re.length;
  // Bit-reversal permutation
  for (let i = 1, j = 0; i < N; i++) {
    let bit = N >> 1;
    for (; j & bit; bit >>= 1) j ^= bit;
    j ^= bit;
    if (i < j) {
      [re[i], re[j]] = [re[j], re[i]];
      [im[i], im[j]] = [im[j], im[i]];
    }
  }
  // Butterfly passes
  for (let len = 2; len <= N; len <<= 1) {
    const half = len >> 1;
    const angle = (-2 * Math.PI) / len;
    const wRe = Math.cos(angle);
    const wIm = Math.sin(angle);
    for (let i = 0; i < N; i += len) {
      let curRe = 1, curIm = 0;
      for (let j = 0; j < half; j++) {
        const tRe = curRe * re[i + j + half] - curIm * im[i + j + half];
        const tIm = curRe * im[i + j + half] + curIm * re[i + j + half];
        re[i + j + half] = re[i + j] - tRe;
        im[i + j + half] = im[i + j] - tIm;
        re[i + j] += tRe;
        im[i + j] += tIm;
        const nextRe = curRe * wRe - curIm * wIm;
        curIm = curRe * wIm + curIm * wRe;
        curRe = nextRe;
      }
    }
  }
}

/** Inverse FFT via conjugate trick */
export function ifft(re: number[], im: number[]): void {
  const N = re.length;
  for (let i = 0; i < N; i++) im[i] = -im[i];
  fft(re, im);
  for (let i = 0; i < N; i++) {
    re[i] /= N;
    im[i] = -im[i] / N;
  }
}

/** Compute power spectrum from FFT output */
export function powerSpectrum(re: number[], im: number[], totalDays: number): SpectrumEntry[] {
  const N = re.length;
  const result: SpectrumEntry[] = [];
  for (let i = 1; i <= N / 2; i++) {
    const magnitude = Math.sqrt(re[i] * re[i] + im[i] * im[i]) / N;
    const period = N / i;
    if (period >= 2 && period <= totalDays / 2) {
      result.push({
        freq: (i / N).toFixed(6),
        period: period.toFixed(1),
        magnitude,
        index: i,
      });
    }
  }
  return result;
}

/** Low-pass filter: keep only low-frequency components */
export function lowPassFilter(signal: number[], cutoffRatio = 0.06): number[] {
  const len = signal.length;
  let N = 1;
  while (N < len) N <<= 1;
  const re = new Array(N).fill(0);
  const im = new Array(N).fill(0);
  for (let i = 0; i < len; i++) re[i] = signal[i];
  fft(re, im);
  const cutoff = Math.floor(N * cutoffRatio);
  for (let i = cutoff; i <= N - cutoff; i++) {
    re[i] = 0;
    im[i] = 0;
  }
  ifft(re, im);
  return re.slice(0, len);
}
