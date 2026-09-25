// Общий рендер ленты диалога: пузыри, чипы балла/стратегии/SPIN.
// Используется живой сессией (pages/session.js) и архивом (pages/dialog.js).

import { h } from './dom.js';
import { badge } from './components.js';
import { t, strategyLabel, spinLabel } from './i18n.js';

const STRATEGY_VARIANT = {
  collaboration: 'success',
  compromise: 'warning',
  confrontation: 'danger',
};

const BUBBLE_BASE = {
  maxWidth: 'min(78%, 640px)',
  padding: '10px 14px',
  borderRadius: 'var(--radius-lg)',
  fontSize: 'var(--fs-base)',
  lineHeight: '1.5',
  whiteSpace: 'pre-wrap',
  wordBreak: 'break-word',
};

const FEED_STYLE = {
  display: 'flex',
  flexDirection: 'column',
  gap: 'var(--sp-3)',
  overflowY: 'auto',
  padding: 'var(--sp-4)',
  minHeight: '280px',
  maxHeight: 'min(58vh, 640px)',
  background: 'var(--surface-2)',
  border: '1px solid var(--border)',
  borderRadius: 'var(--radius-lg)',
};

export function deltaChip(delta) {
  const n = Number(delta) || 0;
  const cls = n > 0 ? 'badge-success' : n < 0 ? 'badge-danger' : 'badge-neutral';
  const label = n > 0 ? `+${n}` : String(n);
  return h(`span.badge.no-dot.${cls}`, { text: t('session.scoreChip', { n: label }) });
}

export function strategyBadge(slug) {
  if (!slug) return null;
  return badge(
    strategyLabel(slug) || slug,
    STRATEGY_VARIANT[slug] || 'neutral',
    { dot: false }
  );
}

export function spinBadge(code) {
  if (!code) return null;
  return h('span.badge.badge-info.no-dot', {
    title: spinLabel(code) || code,
    text: `SPIN ${code}`,
  });
}

/**
 * Пузырь реплики.
 * @param {object} msg — SessionMessage { id, role, content, strategy, score_delta }
 * @param {{ partnerName?: string, spin?: string|null }} [opts]
 */
export function messageNode(msg, { partnerName = '', spin = null } = {}) {
  const isPlayer = msg.role === 'player';
  const wrap = h('div', {
    // msgId нужен странице сессии для ветвления (форк по реплике).
    dataset: { role: msg.role, msgId: msg.id ? String(msg.id) : '' },
    style: {
      display: 'flex',
      justifyContent: isPlayer ? 'flex-end' : 'flex-start',
    },
  });

  const meta = [];
  if (isPlayer) {
    if (typeof msg.score_delta === 'number' && msg.score_delta !== 0) meta.push(deltaChip(msg.score_delta));
    const sb = strategyBadge(msg.strategy);
    if (sb) meta.push(sb);
    if (spin) meta.push(spinBadge(spin));
  }

  const bubble = h('div', {
    style: {
      ...BUBBLE_BASE,
      background: isPlayer ? 'var(--accent-soft)' : 'var(--surface-3)',
      border: `1px solid ${isPlayer ? 'color-mix(in srgb, var(--accent) 35%, transparent)' : 'var(--border)'}`,
      borderLeft: isPlayer ? '4px solid var(--accent)' : '4px solid var(--border-strong)',
      textAlign: 'left',
    },
  },
    !isPlayer && partnerName
      ? h('div.small.muted', { text: partnerName, style: { marginBottom: '4px' } })
      : null,
    h('div', { text: msg.content }),
    meta.length
      ? h('div.row', { style: { gap: 'var(--sp-1)', marginTop: '6px' } }, ...meta)
      : null
  );

  wrap.append(bubble);
  return wrap;
}

/** Стилизованный контейнер ленты (скролл, фон, рамка). */
export function createChatFeed({ id = 'chat-feed' } = {}) {
  return h('div', { id, style: { ...FEED_STYLE } });
}
