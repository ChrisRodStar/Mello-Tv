import type { ServiceHealth } from '../../model';

import { Icon } from './Icon';

type Props = {
  health: ServiceHealth | null;
  checking: boolean;
  onRetry: () => void;
};

export function ServiceUnavailable({ health, checking, onRetry }: Props) {
  return (
    <section className="service-unavailable" aria-labelledby="service-title">
      <div className="service-unavailable-content">
        <h1 id="service-title">Service unavailable</h1>
        <dl
          className="service-checks"
          aria-label="Service health"
          aria-live="polite"
          aria-busy={checking}
        >
          <div>
            <dt>Stream TV API</dt>
            <dd data-state={checking ? 'checking' : health ? 'ok' : 'down'}>
              {checking ? 'Checking…' : health ? 'Available' : 'Unreachable'}
            </dd>
          </div>
          <div>
            <dt>Saved catalog</dt>
            <dd
              data-state={
                checking
                  ? 'checking'
                  : health?.catalog === 'retained'
                    ? 'ok'
                    : health
                      ? 'down'
                      : 'unknown'
              }
            >
              {checking
                ? 'Checking…'
                : health
                  ? health.catalog === 'retained'
                    ? 'Available'
                    : 'No saved titles'
                  : 'Unable to check'}
            </dd>
          </div>
        </dl>
        <button className="service-retry" onClick={onRetry} disabled={checking}>
          <Icon name="refresh" />
          <span>{checking ? 'Checking…' : 'Try again'}</span>
        </button>
      </div>
    </section>
  );
}
