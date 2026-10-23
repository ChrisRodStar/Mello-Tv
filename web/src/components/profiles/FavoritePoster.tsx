import { useEffect, useState } from 'react';

export function FavoritePoster({ src }: { src: string }) {
  const [attempt, setAttempt] = useState(0);
  const [loaded, setLoaded] = useState(false);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    if (!failed || attempt >= 3) return;
    const retry = setTimeout(() => {
      setFailed(false);
      setAttempt((value) => value + 1);
    }, 5000);
    return () => clearTimeout(retry);
  }, [failed, attempt]);
  return (
    <img
      key={`${src}:${attempt}`}
      src={src}
      alt=""
      decoding="async"
      style={{ opacity: loaded ? 1 : 0 }}
      onLoad={() => {
        setLoaded(true);
        setFailed(false);
      }}
      onError={() => {
        setLoaded(false);
        setFailed(true);
      }}
    />
  );
}
