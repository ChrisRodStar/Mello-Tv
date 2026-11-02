type Level = { width?: number; height?: number; bitrate?: number };

function standardQualityHeight(level: Level): 360 | 480 | 720 | 1080 | null {
  const { width, height } = level;
  if (height === 1080 || height === 720 || height === 480 || height === 360) return height;
  if (typeof height === 'number' && height > 0) {
    if (width === 1920 && height <= 1080) return 1080;
    if (width === 1280 && height <= 720) return 720;
    if ((width === 854 || width === 852 || width === 848) && height <= 480) return 480;
    if (width === 640 && height <= 360) return 360;
    if (height >= 1064 && height <= 1080) return 1080;
    if (height >= 704 && height <= 720) return 720;
    if (height >= 464 && height <= 480) return 480;
    if (height >= 344 && height <= 360) return 360;
  }
  return null;
}

/** Expose standard quality tiers declared in the manifest, including cropped widescreen encodes. */
export function videoQualityOptions(
  levels: readonly Level[],
  fallbackLevels: readonly Level[] = [],
) {
  const byHeight = new Map<number, { id: number; bitrate: number }>();
  const collect = (items: readonly Level[], fixedId?: number) => {
    items.forEach((level, index) => {
      const height = standardQualityHeight(level);
      if (!height) return;
      const id = fixedId ?? index;
      const bitrate = level.bitrate ?? 0;
      const previous = byHeight.get(height);
      if (!previous || bitrate > previous.bitrate) byHeight.set(height, { id, bitrate });
    });
  };
  collect(levels);
  if (byHeight.size === 0 && fallbackLevels.length > 0) {
    collect(fallbackLevels, levels.length === 1 ? 0 : undefined);
  }
  return Array.from(byHeight, ([height, level]) => ({
    id: level.id,
    label: `${height}p`,
    height,
  }))
    .sort((a, b) => b.height - a.height)
    .map(({ id, label }) => ({ id, label }));
}
