import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type PointerEvent,
  type TouchEvent,
} from 'react';

import type { Title } from '../../model';

import { FeaturedPoster } from './FeaturedPoster';

const ADVANCE_INTERVAL_MS = 8000;

function modIndex(slot: number, count: number): number {
  if (count <= 0) return 0;
  return ((slot % count) + count) % count;
}

export const FeaturedCarousel = memo(function FeaturedCarousel({
  titles,
  onSelect,
  onPlay,
}: {
  titles: Title[];
  onSelect: (title: Title) => void;
  onPlay?: (title: Title) => void;
}) {
  const featured = useMemo(() => titles.slice(0, 7), [titles]);
  const count = featured.length;

  const [position, setPosition] = useState(0);
  const [dragProgress, setDragProgress] = useState(0);
  const [isDragging, setIsDragging] = useState(false);
  const [paused, setPaused] = useState(false);
  const [stageMetrics, setStageMetrics] = useState(() => ({
    stageWidth: typeof window !== 'undefined' ? window.innerWidth : 1280,
    isCompact: typeof window !== 'undefined' ? window.innerWidth <= 700 : false,
  }));

  const stageRef = useRef<HTMLDivElement>(null);
  const paginationRef = useRef<HTMLDivElement>(null);
  const suppressTapUntilRef = useRef(0);
  const pausedProgressRef = useRef(0);
  const advanceStartRef = useRef<number | null>(null);

  const dragStateRef = useRef<{
    pointerId: number | null;
    startX: number;
    startY: number;
    lastX: number;
    lastTime: number;
    velocityX: number;
    lockedHorizontal: boolean;
    aborted: boolean;
  } | null>(null);

  const currentIndex = modIndex(position, count);

  // Observe stage width so cardWidth and stepX match FeaturedCarouselLayout 1:1
  useLayoutEffect(() => {
    const el = stageRef.current;
    if (!el) return;

    const measure = () => {
      const rect = el.getBoundingClientRect();
      const width = rect.width || window.innerWidth || 1280;
      const isCompact = width <= 700 || window.innerWidth <= window.innerHeight;
      setStageMetrics((prev) =>
        Math.abs(prev.stageWidth - width) < 0.5 && prev.isCompact === isCompact
          ? prev
          : { stageWidth: width, isCompact },
      );
    };

    measure();
    if (typeof ResizeObserver !== 'undefined') {
      const observer = new ResizeObserver(measure);
      observer.observe(el);
      return () => observer.disconnect();
    }
    window.addEventListener('resize', measure);
    return () => window.removeEventListener('resize', measure);
  }, []);

  const { stageWidth, isCompact } = stageMetrics;

  // Center card size: width clamp(210px, 24vw, 286px), aspect-ratio 2 / 3
  const cardWidth = useMemo(() => {
    const base = Math.min(286, Math.max(210, stageWidth * 0.24));
    return Math.round(Math.min(base, stageWidth * 0.7));
  }, [stageWidth]);

  const cardHeight = useMemo(() => Math.round(cardWidth * 1.5), [cardWidth]);

  // Step spacing: min(cardWidth * 0.64, (stageWidth - cardWidth * 0.70) / 6)
  const stepX = useMemo(() => {
    return Math.min(
      cardWidth * 0.64,
      Math.max(1, (stageWidth - cardWidth * 0.7) / 6),
    );
  }, [cardWidth, stageWidth]);

  const swipeThreshold = useMemo(() => Math.max(80, cardWidth * 0.49), [cardWidth]);

  const resetProgressFill = useCallback((value = 0) => {
    pausedProgressRef.current = value;
    advanceStartRef.current = performance.now() - value * ADVANCE_INTERVAL_MS;
    if (paginationRef.current) {
      paginationRef.current.style.setProperty(
        '--indicator-progress',
        `${(value * 100).toFixed(2)}%`,
      );
    }
  }, []);

  const stepBy = useCallback(
    (delta: number) => {
      if (count <= 1 || delta === 0) return;
      resetProgressFill(0);
      setPosition((prev) => prev + delta);
    },
    [count, resetProgressFill],
  );

  const jumpToCardIndex = useCallback(
    (targetIndex: number) => {
      if (count <= 1) return;
      setPosition((prev) => {
        const current = modIndex(prev, count);
        let diff = (targetIndex - current + count) % count;
        if (diff > count / 2) diff -= count;
        return prev + diff;
      });
      resetProgressFill(0);
    },
    [count, resetProgressFill],
  );

  const jumpToSlot = useCallback(
    (targetSlot: number) => {
      if (count <= 1) return;
      resetProgressFill(0);
      setPosition(targetSlot);
    },
    [count, resetProgressFill],
  );

  // Reset position cleanly if the featured titles list identity changes
  const titleKey = useMemo(
    () => featured.map((t) => `${t.type}-${t.id}`).join(','),
    [featured],
  );
  useEffect(() => {
    setDragProgress(0);
    setIsDragging(false);
    resetProgressFill(0);
  }, [titleKey, resetProgressFill]);

  // 8000ms auto-advance timer + live Studio Blue progress fill
  useEffect(() => {
    if (count <= 1) return;
    const reduceMotion =
      typeof window !== 'undefined' &&
      typeof window.matchMedia === 'function' &&
      window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    if (reduceMotion) return;

    if (paused || isDragging) {
      if (advanceStartRef.current !== null) {
        const elapsed = performance.now() - advanceStartRef.current;
        pausedProgressRef.current = Math.min(1, Math.max(0, elapsed / ADVANCE_INTERVAL_MS));
        advanceStartRef.current = null;
      }
      return;
    }

    let rafId = 0;
    const startOffsetMs = pausedProgressRef.current * ADVANCE_INTERVAL_MS;
    let cycleStart = performance.now() - startOffsetMs;
    advanceStartRef.current = cycleStart;

    const tick = (now: number) => {
      if (typeof document !== 'undefined' && document.hidden) {
        cycleStart = now - pausedProgressRef.current * ADVANCE_INTERVAL_MS;
        advanceStartRef.current = cycleStart;
        rafId = requestAnimationFrame(tick);
        return;
      }

      const elapsed = now - cycleStart;
      if (elapsed >= ADVANCE_INTERVAL_MS) {
        pausedProgressRef.current = 0;
        cycleStart = now;
        advanceStartRef.current = now;
        if (paginationRef.current) {
          paginationRef.current.style.setProperty('--indicator-progress', '0%');
        }
        setPosition((prev) => prev + 1);
      } else {
        const ratio = Math.min(1, Math.max(0, elapsed / ADVANCE_INTERVAL_MS));
        pausedProgressRef.current = ratio;
        if (paginationRef.current) {
          paginationRef.current.style.setProperty(
            '--indicator-progress',
            `${(ratio * 100).toFixed(2)}%`,
          );
        }
      }
      rafId = requestAnimationFrame(tick);
    };

    rafId = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(rafId);
  }, [count, paused, isDragging, position]);

  // Visible stage slots: 7 visible slots (-3..+3) plus 1 offstage buffer slot (-4, +4) for smooth wrap fade
  const visibleSlots = useMemo(() => {
    if (count <= 1) return [position];
    const slots: number[] = [];
    for (let offset = -4; offset <= 4; offset += 1) {
      slots.push(position + offset);
    }
    return slots;
  }, [count, position]);

  // Indicator slots (-4..+4 around active position, matching pageIndicator in FeaturedCarouselView.swift)
  const indicatorSlots = useMemo(() => {
    if (count <= 1) return [];
    const slots: number[] = [];
    for (let offset = -4; offset <= 4; offset += 1) {
      slots.push(position + offset);
    }
    return slots;
  }, [count, position]);

  const finishGesture = useCallback(
    (dx: number, velocityX: number) => {
      const triggerDistance = swipeThreshold * 0.35;
      let stepChange = 0;
      if (dx < -triggerDistance || velocityX < -250) {
        stepChange = 1;
      } else if (dx > triggerDistance || velocityX > 250) {
        stepChange = -1;
      }

      suppressTapUntilRef.current = Date.now() + 260;
      setIsDragging(false);
      setDragProgress(0);
      if (stepChange !== 0) {
        resetProgressFill(0);
        setPosition((prev) => prev + stepChange);
      }
    },
    [swipeThreshold, resetProgressFill],
  );

  const handlePointerDown = useCallback(
    (event: PointerEvent<HTMLDivElement>) => {
      if (count <= 1) return;
      if (event.pointerType === 'mouse' && event.button !== 0) return;
      const now = performance.now();
      dragStateRef.current = {
        pointerId: event.pointerId,
        startX: event.clientX,
        startY: event.clientY,
        lastX: event.clientX,
        lastTime: now,
        velocityX: 0,
        lockedHorizontal: false,
        aborted: false,
      };
    },
    [count],
  );

  const handlePointerMove = useCallback(
    (event: PointerEvent<HTMLDivElement>) => {
      const state = dragStateRef.current;
      if (!state || state.aborted || state.pointerId !== event.pointerId) return;

      const dx = event.clientX - state.startX;
      const dy = event.clientY - state.startY;

      if (!state.lockedHorizontal) {
        if (Math.abs(dx) < 10 && Math.abs(dy) < 10) return;
        if (Math.abs(dx) > Math.abs(dy) * 1.25) {
          state.lockedHorizontal = true;
          setIsDragging(true);
          suppressTapUntilRef.current = Date.now() + 260;
          try {
            event.currentTarget.setPointerCapture(event.pointerId);
          } catch {
            // Ignore capture errors on synthetic events
          }
        } else {
          state.aborted = true;
          return;
        }
      }

      const now = performance.now();
      const dt = Math.max(1, now - state.lastTime);
      state.velocityX = ((event.clientX - state.lastX) / dt) * 1000;
      state.lastX = event.clientX;
      state.lastTime = now;

      const clampedDx = Math.max(-swipeThreshold, Math.min(swipeThreshold, dx));
      setDragProgress(clampedDx / swipeThreshold);
    },
    [swipeThreshold],
  );

  const handlePointerUpOrCancel = useCallback(
    (event: PointerEvent<HTMLDivElement>) => {
      const state = dragStateRef.current;
      if (!state || state.pointerId !== event.pointerId) return;
      dragStateRef.current = null;

      if (!state.lockedHorizontal) return;
      const dx = event.clientX - state.startX;
      finishGesture(dx, state.velocityX);
    },
    [finishGesture],
  );

  // Touch fallback for environments sending touch events directly
  const touchStartXRef = useRef<number | null>(null);
  const touchStartYRef = useRef<number | null>(null);

  const handleTouchStart = useCallback(
    (event: TouchEvent<HTMLDivElement>) => {
      if (count <= 1 || dragStateRef.current) return;
      const touch = event.touches[0];
      if (!touch) return;
      touchStartXRef.current = touch.clientX;
      touchStartYRef.current = touch.clientY;
    },
    [count],
  );

  const handleTouchMove = useCallback(
    (event: TouchEvent<HTMLDivElement>) => {
      if (count <= 1 || dragStateRef.current || touchStartXRef.current === null) return;
      const touch = event.touches[0];
      if (!touch) return;
      const dx = touch.clientX - touchStartXRef.current;
      const dy = touch.clientY - (touchStartYRef.current ?? touch.clientY);
      if (!isDragging && Math.abs(dx) > 10 && Math.abs(dx) > Math.abs(dy) * 1.25) {
        setIsDragging(true);
      }
      if (isDragging || (Math.abs(dx) > 10 && Math.abs(dx) > Math.abs(dy) * 1.25)) {
        const clamped = Math.max(-swipeThreshold, Math.min(swipeThreshold, dx));
        setDragProgress(clamped / swipeThreshold);
      }
    },
    [count, isDragging, swipeThreshold],
  );

  const handleTouchEnd = useCallback(
    (event: TouchEvent<HTMLDivElement>) => {
      if (touchStartXRef.current === null) return;
      const startX = touchStartXRef.current;
      touchStartXRef.current = null;
      touchStartYRef.current = null;
      const endTouch = event.changedTouches[0];
      if (!endTouch) return;
      const dx = endTouch.clientX - startX;
      if (isDragging || Math.abs(dx) > 36) {
        finishGesture(dx, 0);
      }
    },
    [isDragging, finishGesture],
  );

  const handleKeyDown = useCallback(
    (event: KeyboardEvent<HTMLElement>) => {
      if (count <= 1) return;
      if (event.key === 'ArrowLeft') {
        event.preventDefault();
        stepBy(-1);
      } else if (event.key === 'ArrowRight') {
        event.preventDefault();
        stepBy(1);
      }
    },
    [count, stepBy],
  );

  if (!count) return null;

  const indicatorSpan = Math.min(
    Math.max(180, stageWidth - 24),
    cardWidth * (isCompact ? 0.96 : 1.1) + stepX * 2,
  );

  return (
    <section
      className={`featured-carousel${isDragging ? ' is-dragging' : ''}`}
      aria-label="Featured titles"
      aria-roledescription="carousel"
      onKeyDown={handleKeyDown}
      onMouseEnter={() => setPaused(true)}
      onMouseLeave={() => setPaused(false)}
      onFocusCapture={() => setPaused(true)}
      onBlurCapture={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget)) {
          setPaused(false);
        }
      }}
      style={
        {
          '--card-width': `${cardWidth}px`,
          '--card-height': `${cardHeight}px`,
          '--step-x': `${stepX.toFixed(2)}px`,
        } as CSSProperties
      }
    >
      <div
        ref={stageRef}
        className="featured-stage"
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUpOrCancel}
        onPointerCancel={handlePointerUpOrCancel}
        onTouchStart={handleTouchStart}
        onTouchMove={handleTouchMove}
        onTouchEnd={handleTouchEnd}
      >
        {/* Showroom floor elliptical spotlight aura centered behind the stage */}
        <div className="featured-floor-aura" aria-hidden="true" />

        {/* 7 visible 3D showroom portrait poster layers (-3 to +3) */}
        <div className="featured-stage-stack">
          {visibleSlots.map((slot) => {
            const itemIndex = modIndex(slot, count);
            const title = featured[itemIndex];
            if (!title) return null;
            const effectiveOffset = slot - position + dragProgress;
            const absOffset = Math.abs(effectiveOffset);
            const depth = Math.min(absOffset, 3);

            const xOffset = effectiveOffset * stepX;
            const scale = 1 - depth * 0.1;
            const rotationY = effectiveOffset * 4;
            const yOffset = isCompact ? -depth * 10 : depth * 2;
            const zIndex = 100 - Math.round(absOffset * 20);
            const opacity = Math.max(0, 1 - Math.max(0, absOffset - 3) * 2);
            const contentOpacity = Math.max(0, 1 - absOffset * 1.8);

            const isCenter = absOffset < 0.35;
            const proximity = Math.max(0, 1 - absOffset);
            const focus = proximity * proximity * (3 - 2 * proximity);
            const posterUrl = title.posterUrl || title.backdropUrl;

            return (
              <article
                key={slot}
                className={`featured-card${isCenter ? ' is-active' : ''}`}
                data-offset={Math.round(effectiveOffset)}
                style={
                  {
                    '--effective-offset': effectiveOffset,
                    '--depth': depth,
                    '--focus': focus,
                    transform: `translate3d(${xOffset.toFixed(2)}px, ${yOffset.toFixed(2)}px, 0) perspective(900px) rotateY(${rotationY.toFixed(2)}deg) scale(${scale.toFixed(4)})`,
                    zIndex,
                    opacity,
                    pointerEvents: absOffset <= 3.1 ? 'auto' : 'none',
                  } as CSSProperties
                }
                aria-hidden={!isCenter}
              >
                {/* Saturated, blurred ambient poster halo behind the active center card */}
                {posterUrl && focus > 0.01 && (
                  <div
                    className="featured-poster-light"
                    style={{ opacity: Number((focus * 0.75).toFixed(3)) }}
                    aria-hidden="true"
                  >
                    <img src={posterUrl} alt="" decoding="async" draggable={false} />
                  </div>
                )}

                <FeaturedPoster
                  title={title}
                  active={isCenter}
                  eager={absOffset <= 1.5}
                  effectiveOffset={effectiveOffset}
                  depth={depth}
                  contentOpacity={contentOpacity}
                  onActivate={() => {
                    if (isDragging || Date.now() < suppressTapUntilRef.current) {
                      return;
                    }
                    if (isCenter) {
                      (onPlay ?? onSelect)(title);
                    } else {
                      jumpToSlot(slot);
                    }
                  }}
                  onDetails={() => {
                    if (isDragging || Date.now() < suppressTapUntilRef.current) {
                      return;
                    }
                    onSelect(title);
                  }}
                />
              </article>
            );
          })}
        </div>
      </div>

      {/* 3D Arc Capsule Page Indicator with Live Studio Blue Progress Fill */}
      {count > 1 && (
        <div
          ref={paginationRef}
          className="featured-pagination"
          style={
            {
              '--indicator-span': `${indicatorSpan.toFixed(1)}px`,
              '--indicator-progress': '0%',
            } as CSSProperties
          }
        >
          <div
            className="featured-dots"
            role="group"
            aria-label={`Featured titles (${currentIndex + 1} of ${count})`}
          >
            {indicatorSlots.map((slot) => {
              const slotTitleIndex = modIndex(slot, count);
              const slotTitle = featured[slotTitleIndex];
              if (!slotTitle) return null;
              const baseOffset = slot - position;
              const distance = baseOffset + dragProgress;
              const absDistance = Math.abs(distance);
              const emphasis = Math.max(0, 1 - absDistance);
              const depth = Math.min(absDistance, 4);

              const capsuleWidth = 56;
              const widthPx = 8 + (capsuleWidth - 8) * emphasis;
              const dotScale = Math.max(0.4, 1 - depth * 0.16);
              const arcX =
                (Math.sin(distance * 0.22) / Math.sin(0.66)) *
                (indicatorSpan / 2 - 8);
              const arcDepth = Math.min(absDistance / 3.0, 1.5);
              const arcY = 5 - arcDepth * arcDepth * 11;
              const rotY = distance * 4;
              const dotOpacity = Math.max(0.35, 1 - depth * 0.15);
              const isCurrentSlot = baseOffset === 0;

              // Expose unique title buttons inside the primary window to assistive tech
              const isPrimaryOccurrence =
                baseOffset >= -Math.floor((count - 1) / 2) &&
                baseOffset <= Math.ceil((count - 1) / 2);

              return (
                <button
                  key={slot}
                  type="button"
                  className={`featured-arc-dot${isCurrentSlot ? ' active' : ''}`}
                  aria-label={`Feature ${slotTitle.title}`}
                  aria-pressed={isCurrentSlot}
                  aria-hidden={!isPrimaryOccurrence ? true : undefined}
                  tabIndex={isPrimaryOccurrence ? 0 : -1}
                  onClick={() => {
                    if (isCurrentSlot) return;
                    if (isPrimaryOccurrence) {
                      jumpToCardIndex(slotTitleIndex);
                    } else {
                      jumpToSlot(slot);
                    }
                  }}
                  style={
                    {
                      width: `${widthPx.toFixed(2)}px`,
                      marginLeft: `${(-widthPx / 2).toFixed(2)}px`,
                      transform: `translate3d(${arcX.toFixed(2)}px, ${arcY.toFixed(2)}px, 0) perspective(420px) rotateY(${rotY.toFixed(2)}deg) scale(${dotScale.toFixed(3)})`,
                      opacity: Number(dotOpacity.toFixed(3)),
                      zIndex: 20 - Math.round(absDistance * 4),
                    } as CSSProperties
                  }
                >
                  <span
                    className="featured-arc-fill"
                    style={{ opacity: Number(emphasis.toFixed(3)) }}
                    aria-hidden="true"
                  />
                </button>
              );
            })}
          </div>
        </div>
      )}
    </section>
  );
});
