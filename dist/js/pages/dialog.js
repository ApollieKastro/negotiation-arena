// Архивный просмотр диалога: read-only лента сохранённых реплик сессии.
// Данные не редактируются — только пересмотр из истории/отчёта.

import { h, fmtDateTime } from '../core/dom.js';
import { emptyState, spinner, badge } from '../core/components.js';
import { request, ApiError } from '../core/api.js';
import { navigate } from '../core/router.js';
import { t, statusLabel } from '../core/i18n.js';
import { messageNode, createChatFeed } from '../core/chat.js';

const STATUS_VARIANT = { active: 'info', finished: 'success', abandoned: 'neutral' };

function header(title, sub) {
  return h('div.page-header', null,
    h('div', null,
      h('h1', { text: title }),
      h('div.page-sub', { text: sub })
    )
  );
}

export function renderPage(root, params = {}) {
  const id = params.id;
  // Guard от гонки: если страницу уже покинули — не дописываем в root
  const marker = h('span', { style: { display: 'none' }, 'data-page': 'dialog' });
  const isCurrent = () => marker.isConnected;

  root.replaceChildren(
    marker,
    header(t('dialog.title'), t('dialog.loading')),
    h('div.card', null, h('div.card-body', null, spinner('lg', { label: t('dialog.loadingSession') })))
  );

  Promise.all([
    request(`/sessions/${id}`),
    request(`/sessions/${id}/messages`),
    request('/scenarios').catch(() => []),
  ]).then(([session, messages, scenarios]) => {
    if (!isCurrent()) return;

    const scenario = (scenarios || []).find((x) => x.id === session.scenario_id) || null;
    const partnerName = scenario ? scenario.partner_name : t('session.partner');
    const isActive = session.status === 'active';
    const isFinished = session.status === 'finished';

    const statusBadge = badge(
      statusLabel(session.status),
      STATUS_VARIANT[session.status] || 'neutral'
    );

    const headerCard = h('div.card', null,
      h('div.card-body.stack', { style: { gap: 'var(--sp-2)' } },
        h('div.row-between', null,
          h('div.stack', { style: { gap: '4px' } },
            h('div.row', null,
              h('strong', { text: scenario ? scenario.title : t('dialog.title') }),
              statusBadge
            ),
            h('div.small.muted', { text: fmtDateTime(session.created_at) })
          ),
          h('div.row', { style: { gap: 'var(--sp-2)', flexWrap: 'wrap' } },
            h('span.small.muted', null, t('session.turns')),
            h('strong.num', { text: String(session.turn_count ?? 0) }),
            h('span.small.muted', null, t('session.score')),
            h('strong.num', { text: String(session.total_score ?? 0) }),
            isActive
              ? h('a.btn.btn-primary.btn-sm', { href: `#/session/${id}` }, t('dialog.continue'))
              : null,
            isFinished
              ? h('a.btn.btn-secondary.btn-sm', { href: `#/result/${id}` }, t('action.result'))
              : null,
            h('a.btn.btn-ghost.btn-sm', { href: '#/history' }, t('action.history'))
          )
        ),
        session.ending_title
          ? h('div.row', null, badge(session.ending_title, 'accent', { dot: false }))
          : null,
        h('div.small.muted', { text: t('dialog.readonly') })
      )
    );

    const feed = createChatFeed({ id: 'dialog-feed' });
    const list = Array.isArray(messages) ? messages : [];
    const feedCard = list.length
      ? (() => {
          feed.replaceChildren(...list.map((m) => messageNode(m, { partnerName })));
          feed.scrollTop = feed.scrollHeight;
          return h('div.mt-4', null, feed);
        })()
      : h('div.mt-4', null, emptyState({
          icon: '∅',
          title: t('dialog.empty'),
          description: t('dialog.emptyDesc'),
        }));

    root.replaceChildren(
      marker,
      header(t('dialog.title'), fmtDateTime(session.created_at)),
      headerCard,
      feedCard,
      h('div.row.mt-4', null,
        h('a.btn.btn-secondary', { href: '#/history' }, t('action.history')),
        isFinished
          ? h('a.btn.btn-primary', { href: `#/result/${id}` }, t('action.result'))
          : null,
        isActive
          ? h('a.btn.btn-primary', { href: `#/session/${id}` }, t('dialog.continue'))
          : null
      )
    );
  }).catch((err) => {
    if (!isCurrent()) return;
    root.replaceChildren(
      marker,
      header(t('dialog.title'), t('dialog.loadError')),
      emptyState({
        icon: '⚠',
        title: t('dialog.openError'),
        description: err instanceof ApiError ? err.message : t('common.error'),
        action: h('button.btn.btn-secondary', {
          type: 'button',
          text: t('action.history'),
          onClick: () => navigate('#/history'),
        }),
      })
    );
  });
}
