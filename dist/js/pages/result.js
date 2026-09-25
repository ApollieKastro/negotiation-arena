// Результат: hero-балл, финал, метрики, SPIN, стратегии, рекомендации

import { h } from '../core/dom.js';
import { emptyState, skeleton, statCard, badge } from '../core/components.js';
import { request, ApiError } from '../core/api.js';
import { navigate } from '../core/router.js';
import { t, strategyLabel, spinLabel } from '../core/i18n.js';

const STRATEGY_VARIANT = {
  collaboration: 'success',
  compromise: 'warning',
  confrontation: 'danger',
};

/** Лёгкий рендер markdown-подобного feedback (##, -, **bold**) */
function renderFeedback(text) {
  const out = h('div.stack', { style: { gap: 'var(--sp-2)' } });
  const lines = String(text || '').split('\n');
  let list = null;
  for (const raw of lines) {
    const line = raw.trimEnd();
    if (!line.trim()) { list = null; continue; }
    if (line.startsWith('## ')) {
      list = null;
      out.append(h('strong', { text: line.slice(3) }));
      continue;
    }
    if (line.startsWith('- ') || line.startsWith('* ')) {
      if (!list) {
        list = h('ul', { style: { margin: '0', paddingLeft: '1.2em' } });
        out.append(list);
      }
      list.append(h('li', { text: line.slice(2).replace(/\*\*(.+?)\*\*/g, '$1') }));
      continue;
    }
    list = null;
    out.append(h('div', {
      text: line.replace(/\*\*(.+?)\*\*/g, '$1'),
      style: { whiteSpace: 'pre-wrap' },
    }));
  }
  return out;
}

function progressRow(label, code, count, max) {
  const pct = max > 0 ? Math.min(100, Math.round((count / max) * 100)) : 0;
  return h('div.stack', { style: { gap: '4px' } },
    h('div.row-between.small', null,
      h('span', null,
        h('strong', { text: code }),
        h('span.muted', { text: ` — ${label}` })
      ),
      h('span.num', { text: String(count) })
    ),
    h('div.progress', null,
      h('div.progress-bar', { style: { width: `${pct}%` } })
    )
  );
}

export function renderPage(root, params = {}) {
  const id = params.id;
  // Guard от гонки: если страницу уже покинули — не дописываем в root
  const marker = h('span', { style: { display: 'none' }, 'data-page': 'result' });
  const isCurrent = () => marker.isConnected;

  const loading = h('div.card', null,
    h('div.card-body', null, skeleton(5, 24))
  );
  root.replaceChildren(
    marker,
    h('div.page-header', null,
      h('div', null,
        h('h1', { text: t('result.title') }),
        h('div.page-sub', { text: t('result.sub') })
      )
    ),
    loading
  );

  request(`/sessions/${id}/report`).then((r) => {
    if (!isCurrent()) return;
    const spin = r.spin_counts || {};
    const spinMax = Math.max(1, Number(r.turn_count) || 1);
    const stratCounts = [
      { key: 'collaboration', value: r.collaboration_count || 0 },
      { key: 'compromise', value: r.compromise_count || 0 },
      { key: 'confrontation', value: r.confrontation_count || 0 },
    ];

    const hero = h('div.card', null,
      h('div.card-body.stack', { style: { gap: 'var(--sp-3)', alignItems: 'center', textAlign: 'center' } },
        h('div.small.muted', { text: t('result.total') }),
        h('div', {
          style: { fontSize: '3rem', fontWeight: '800', lineHeight: '1', color: 'var(--accent)' },
          class: 'num',
          text: String(r.total_score),
        }),
        h('div.card-title', { text: r.ending ? r.ending.title : t('result.endingFallback') }),
        h('div', {
          text: r.ending ? r.ending.text : '',
          style: { color: 'var(--muted)', maxWidth: '560px' },
        }),
        r.ending && r.ending.outcome
          ? badge(r.ending.outcome, 'accent', { dot: false })
          : null,
        (r.total_score || 0) > 0
          ? badge(`+${Math.max(0, r.total_score)} XP`, 'success', { dot: false })
          : null
      )
    );

    const metrics = h('div.stat-grid.mt-4', null,
      statCard({ label: t('result.strategy'), value: r.strategy_score }),
      statCard({ label: t('result.argument'), value: r.argument_score }),
      statCard({ label: t('result.tone'), value: r.tone_score }),
      statCard({ label: t('result.techniqueBonus'), value: r.technique_bonus })
    );

    const extra = h('div.stat-grid.mt-3', null,
      statCard({ label: t('result.turns'), value: r.turn_count }),
      statCard({ label: t('result.interestFocus'), value: r.interest_focused }),
      statCard({ label: t('result.objectiveCriteria'), value: r.objective_criteria_used })
    );

    const spinCard = h('div.card', null,
      h('div.card-body.stack', { style: { gap: 'var(--sp-3)' } },
        h('div.card-title', { text: t('result.spin') }),
        progressRow(spinLabel('S'), 'S', spin.situation || 0, spinMax),
        progressRow(spinLabel('P'), 'P', spin.problem || 0, spinMax),
        progressRow(spinLabel('I'), 'I', spin.implication || 0, spinMax),
        progressRow(spinLabel('N'), 'N', spin.need_payoff || 0, spinMax)
      )
    );

    const stratCard = h('div.card', null,
      h('div.card-body.stack', { style: { gap: 'var(--sp-3)' } },
        h('div.card-title', { text: t('result.strategies') }),
        ...stratCounts.map((s) => h('div.row-between', null,
          badge(strategyLabel(s.key), STRATEGY_VARIANT[s.key]),
          h('strong.num', { text: String(s.value) })
        ))
      )
    );

    const goalFeedback = h('div.card.mt-4', null,
      h('div.card-body.stack', { style: { gap: 'var(--sp-4)' } },
        h('div.stack', { style: { gap: 'var(--sp-2)' } },
          h('div.card-title', { text: t('result.goal') }),
          h('div', { text: r.goal || '—', style: { color: 'var(--muted)' } })
        ),
        h('div.divider'),
        h('div.stack', { style: { gap: 'var(--sp-2)' } },
          h('div.card-title', { text: t('result.feedback') }),
          renderFeedback(r.feedback)
        )
      )
    );

    const recs = r.recommendations && r.recommendations.length
      ? h('div.card.mt-4', null,
          h('div.card-body.stack', { style: { gap: 'var(--sp-3)' } },
            h('div.card-title', { text: t('result.recommendations') }),
            h('ul', { style: { margin: '0', paddingLeft: '1.2em', display: 'grid', gap: 'var(--sp-2)' } },
              ...r.recommendations.map((x) => h('li', { text: x }))
            )
          )
        )
      : null;

    root.replaceChildren(
      marker,
      h('div.page-header', null,
        h('div', null,
          h('h1', { text: t('result.title') }),
          h('div.page-sub', null, t('result.sub'))
        ),
        h('div.page-actions', null,
          h('a.btn.btn-secondary', { href: `#/dialog/${id}` }, t('action.dialog')),
          h('a.btn.btn-primary', { href: '#/scenarios' }, t('action.playAgain')),
          h('a.btn.btn-ghost', { href: '#/' }, t('action.home'))
        )
      ),
      hero,
      metrics,
      extra,
      h('div.grid-2.mt-4', null, spinCard, stratCard),
      goalFeedback,
      recs,
      h('div.row.mt-4', null,
        h('a.btn.btn-secondary', { href: `#/dialog/${id}` }, t('action.dialog')),
        h('a.btn.btn-primary', { href: '#/scenarios' }, t('action.playAgain')),
        h('a.btn.btn-secondary', { href: '#/' }, t('action.home'))
      )
    );
  }).catch((err) => {
    if (!isCurrent()) return;
    root.replaceChildren(
      marker,
      h('div.page-header', null,
        h('div', null,
          h('h1', { text: t('result.title') }),
          h('div.page-sub', { text: t('result.loadError') })
        )
      ),
      emptyState({
        icon: '⚠',
        title: t('result.reportError'),
        description: err instanceof ApiError ? err.message : t('common.error'),
        action: h('button.btn.btn-secondary', {
          type: 'button',
          text: t('action.home'),
          onClick: () => navigate('#/'),
        }),
      })
    );
  });
}
