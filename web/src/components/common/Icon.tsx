export function Icon({
  name,
}: {
  name:
    | 'switch'
    | 'refresh'
    | 'search'
    | 'close'
    | 'left'
    | 'right'
    | 'info'
    | 'plus'
    | 'play'
    | 'down'
    | 'pause'
    | 'rewind'
    | 'forward'
    | 'volume'
    | 'muted'
    | 'expand'
    | 'collapse';
}) {
  const paths = {
    switch: 'M4 7h16m-4-4 4 4-4 4M20 17H4m4-4-4 4 4 4',
    pause: 'M8 5v14M16 5v14',
    rewind: 'M3 10V4m0 6h6M3 10a9 9 0 1 1 1 8',
    forward: 'M21 10V4m0 6h-6M21 10a9 9 0 1 0-1 8',
    volume: 'M3 9h4l5-4v14l-5-4H3zM16 8a6 6 0 0 1 0 8M19 5a10 10 0 0 1 0 14',
    muted: 'M3 9h4l5-4v14l-5-4H3zM17 9l5 6M17 15l5-6',
    expand: 'M8 3H3v5M16 3h5v5M3 16v5h5M21 16v5h-5',
    collapse: 'M3 8h5V3M21 8h-5V3M8 21v-5H3M16 21v-5h5',
    play: 'm9 5 11 7-11 7z',
    down: 'm6 9 6 6 6-6',
    refresh:
      'M20 7v5h-5M4 17v-5h5M6.1 7a7 7 0 0 1 11.55-1.65L20 8M4 16l2.35 2.65A7 7 0 0 0 17.9 17',
    plus: 'M12 5v14M5 12h14',
    search: 'M21 21l-5-5M18 10a8 8 0 1 1-16 0 8 8 0 0 1 16 0',
    close: 'M6 6l12 12M6 18L18 6',
    left: 'M15 5l-7 7 7 7',
    right: 'M9 5l7 7-7 7',
    info: 'M12 11v6M12 7h.01M22 12a10 10 0 1 1-20 0 10 10 0 0 1 20 0',
  };
  return (
    <svg
      viewBox="0 0 24 24"
      fill={name === 'play' ? 'currentColor' : 'none'}
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d={paths[name]} />
    </svg>
  );
}
