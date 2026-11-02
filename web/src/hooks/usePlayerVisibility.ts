import { useCallback, useEffect, useRef, useState } from 'react';

export function usePlayerVisibility(canHide: boolean) {
  const [visible, setVisible] = useState(true);
  const [focused, updateFocused] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const allowed = useRef(false);
  allowed.current = canHide && !focused;
  const wake = useCallback(() => {
    setVisible(true);
    if (timer.current) clearTimeout(timer.current);
    if (allowed.current)
      timer.current = setTimeout(() => {
        if (allowed.current) setVisible(false);
      }, 2400);
  }, []);
  function toggle() {
    if (visible) {
      if (timer.current) clearTimeout(timer.current);
      setVisible(false);
    } else wake();
  }
  const setFocused = useCallback(
    (value: boolean) => {
      updateFocused(value);
      if (value) wake();
    },
    [wake],
  );
  useEffect(() => {
    if (!canHide) setVisible(true);
    if (canHide && visible && !focused)
      timer.current = setTimeout(() => {
        if (allowed.current) setVisible(false);
      }, 2400);
    return () => {
      if (timer.current) clearTimeout(timer.current);
    };
  }, [canHide, focused, visible, wake]);
  return {
    visible,
    wake,
    toggle,
    setFocused,
  };
}
