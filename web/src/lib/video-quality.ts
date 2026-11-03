type Level = { width?: number; height?: number; bitrate?: number };

function standardQualityHeight(level: Level): 360 | 480 | 720 | 1080 | null {
  const { width, height } = level;
  if (height === 1080 || height === 720 || height === 480 || height === 360) return height;
  if (typeof height === 'number' && height > 0) {
    if (typeof width === 'number' && width > 0) {
      if (width >= 1904 && width <= 1920 && height <= 1088) return 1080;
      if (width >= 1264 && width <= 1280 && height <= 728) return 720;
      if (width >= 840 && width <= 864 && height <= 488) return 480;
      if (width >= 624 && width <= 648 && height <= 368) return 360;
    }
    if (height >= 1060 && height <= 1088) return 1080;
    if (height >= 700 && height <= 728) return 720;
    if (height >= 460 && height <= 488) return 480;
    if (height >= 340 && height <= 368) return 360;
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
  if (byHeight.size === 0 && fallbackLevels.length > 0 && levels.length <= 1) {
    collect(fallbackLevels, 0);
  }
  return Array.from(byHeight, ([height, level]) => ({
    id: level.id,
    label: `${height}p`,
    height,
  }))
    .sort((a, b) => b.height - a.height)
    .map(({ id, label }) => ({ id, label }));
}
