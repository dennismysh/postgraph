import type { Post } from '$lib/api';
import { fft, powerSpectrum, lowPassFilter, type SpectrumEntry } from '$lib/fft';

export type CadenceEntry = {
  date: string;
  posts: number;
};

export type HourlyEntry = {
  hour: number;
  count: number;
};

/** Count posts per day, gap-filled with honest zeros (no posts = 0 posts) */
export function postsToCadence(posts: Post[]): CadenceEntry[] {
  const map = new Map<string, number>();
  for (const p of posts) {
    const date = p.timestamp.slice(0, 10);
    map.set(date, (map.get(date) ?? 0) + 1);
  }
  const dates = [...map.keys()].sort();
  if (dates.length === 0) return [];

  const result: CadenceEntry[] = [];
  const start = new Date(dates[0]);
  const end = new Date(dates[dates.length - 1]);
  for (let d = new Date(start); d <= end; d.setDate(d.getDate() + 1)) {
    const key = d.toISOString().slice(0, 10);
    result.push({ date: key, posts: map.get(key) ?? 0 });
  }
  return result;
}

/** Aggregate posts into 24 hourly buckets */
export function postsToHourly(posts: Post[]): HourlyEntry[] {
  const buckets = Array.from({ length: 24 }, (_, i) => ({ hour: i, count: 0 }));
  for (const p of posts) {
    const hour = new Date(p.timestamp).getHours();
    buckets[hour].count += 1;
  }
  return buckets;
}

/** Compute power spectrum with DC removal and zero-padding */
export function computeSpectrum(signal: number[]): SpectrumEntry[] {
  const totalDays = signal.length;
  const mean = signal.reduce((a, b) => a + b, 0) / totalDays;
  const centered = signal.map(v => v - mean);

  let N = 1;
  while (N < totalDays) N <<= 1;
  const re = new Array(N).fill(0);
  const im = new Array(N).fill(0);
  for (let i = 0; i < totalDays; i++) re[i] = centered[i];

  fft(re, im);
  return powerSpectrum(re, im, totalDays);
}

/** Compute smoothed trend via low-pass filter */
export function computeSmoothed(signal: number[], cutoffRatio = 0.06): number[] {
  return lowPassFilter(signal, cutoffRatio);
}

/** Return top n spectrum entries by magnitude */
export function topPeaks(spectrum: SpectrumEntry[], n: number): SpectrumEntry[] {
  return [...spectrum].sort((a, b) => b.magnitude - a.magnitude).slice(0, n);
}

export type { SpectrumEntry };
