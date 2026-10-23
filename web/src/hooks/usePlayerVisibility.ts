import { useCallback, useEffect, useRef, useState } from 'react';

export function usePlayerVisibility(canHide: boolean) {
  const [visible, setVisible] = useState(true);
  const [focused, setFocused] = useState(false);
  const [hovered, setHovered] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const allowed = useRef(false);
  allowed.current = canHide && !focused && !hovered;
  const wake = useCallback(() => {
    setVisible(true);
    if (timer.current) clearTimeout(timer.current);
    if (allowed.current) timer.current = setTimeout(() => setVisible(false), 2400);
  }, []);
  function toggle() {
    if (visible) {
      if (timer.current) clearTimeout(timer.current);
      setFocused(false);
      setHovered(false);
      setVisible(false);
    } else wake();
  }
  useEffect(() => {
    if (canHide && visible && !focused && !hovered)
      timer.current = setTimeout(() => setVisible(false), 2400);
    return () => {
      if (timer.current) clearTimeout(timer.current);
    };
  }, [canHide, focused, hovered, visible, wake]);
  return {
    visible,
    wake,
    toggle,
    setFocused,
    setHovered,
  };
}
