import { describe, expect, test } from 'bun:test';
import { videoQualityOptions } from '../src/lib/video-quality';

describe('video quality options', () => {
  test('offers only standard heights declared by the manifest, highest first', () => {
    expect(
      videoQualityOptions([
        { height: 180, bitrate: 50 },
        { height: 360, bitrate: 100 },
        { height: 720, bitrate: 200 },
        { height: 1080, bitrate: 400 },
        { height: 480, bitrate: 150 },
        { height: 800, bitrate: 300 },
        {},
      ]),
    ).toEqual([
      { id: 3, label: '1080p' },
      { id: 2, label: '720p' },
      { id: 4, label: '480p' },
      { id: 1, label: '360p' },
    ]);
  });
  test('deduplicates by resolution and selects the highest bitrate rendition', () => {
    expect(
      videoQualityOptions([
        { height: 720, bitrate: 100 },
        { height: 720, bitrate: 300 },
        { height: 720, bitrate: 200 },
      ]),
    ).toEqual([{ id: 1, label: '720p' }]);
  });
  test('does not infer a resolution from bitrate or a cropped height', () => {
    expect(videoQualityOptions([{ bitrate: 1000000 }, { height: 800 }, { height: 0 }])).toEqual(
      [],
    );
  });
});
