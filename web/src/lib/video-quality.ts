type Level = { height?: number; bitrate?: number };

/** Only expose resolutions actually declared in the manifest. */
export function videoQualityOptions(levels: readonly Level[]) {
  const byHeight = new Map<number, { id: number; bitrate: number }>();
  levels.forEach((level, id) => {
    const height = level.height;
    if (height !== 360 && height !== 480 && height !== 720 && height !== 1080) return;
    const bitrate = level.bitrate ?? 0;
    const previous = byHeight.get(height);
    if (!previous || bitrate > previous.bitrate) byHeight.set(height, { id, bitrate });
  });
  return Array.from(byHeight, ([height, level]) => ({
    id: level.id,
    label: `${height}p`,
    height,
  }))
    .sort((a, b) => b.height - a.height)
    .map(({ id, label }) => ({ id, label }));
}
