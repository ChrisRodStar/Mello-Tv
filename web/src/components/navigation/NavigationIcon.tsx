export function NavigationIcon({ name }: { name: 'home' | 'movie' | 'tv' | 'search' }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.7"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {name === 'home' && (
        <path d="m3 10 7.7-6.3a2 2 0 0 1 2.6 0L21 10M5 8.5V19a2 2 0 0 0 2 2h3v-7h4v7h3a2 2 0 0 0 2-2V8.5" />
      )}
      {name === 'movie' && (
        <>
          <path d="M3 9h18v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V9Zm0 0-.7-3.5a1.5 1.5 0 0 1 1.2-1.8l15-2.6A1.5 1.5 0 0 1 20.2 2l.8 4.1L3 9ZM8 3l2 4.8M15 1.8l2 4.8" />
          <path d="m10 13 5 2.5-5 2.5z" />
        </>
      )}
      {name === 'tv' && (
        <>
          <rect x="3" y="4" width="18" height="13" rx="2.5" />
          <path d="M8 21h8M12 17v4" />
        </>
      )}
      {name === 'search' && (
        <>
          <circle cx="10.5" cy="10.5" r="7" />
          <path d="m16 16 4.5 4.5" />
        </>
      )}
    </svg>
  );
}
