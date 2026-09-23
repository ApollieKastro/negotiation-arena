// Страница входа/регистрации — полностью рабочая

import { h } from '../core/dom.js';
import { field, toast } from '../core/components.js';
import { request, ApiError } from '../core/api.js';
import { setAuth } from '../core/store.js';
import { navigate } from '../core/router.js';
import { t, getLocale, setLocale } from '../core/i18n.js';

/** Переключатель RU/EN на голой auth-странице (до входа). */
function langSwitch(onChange) {
  const wrap = h('div.auth-lang', {
    role: 'group',
    'aria-label': t('common.langSwitch'),
  });
  for (const code of ['ru', 'en']) {
    const active = getLocale() === code;
    const b = h('button.auth-lang-btn', {
      type: 'button',
      text: code.toUpperCase(),
      'aria-pressed': active ? 'true' : 'false',
      'aria-label': code === 'ru' ? t('settings.languageRu') : t('settings.languageEn'),
      title: code === 'ru' ? t('settings.languageRu') : t('settings.languageEn'),
      onClick: () => {
        if (getLocale() === code) return;
        setLocale(code);
        try { localStorage.setItem('na.locale', code); } catch { /* ignore */ }
        try {
          const s = JSON.parse(localStorage.getItem('na.settings') || '{}');
          localStorage.setItem('na.settings', JSON.stringify({ ...s, locale: code }));
        } catch { /* ignore */ }
        window.dispatchEvent(new CustomEvent('locale-changed', { detail: { locale: code } }));
        onChange();
      },
    });
    if (active) b.classList.add('is-active');
    wrap.append(b);
  }
  return wrap;
}

export function renderPage(root, params = {}) {
  let mode = params.mode === 'register' ? 'register' : 'login';

  const errorBox = h('div.form-error', { role: 'alert' });
  const formHost = h('div.auth-form');

  const tabsEl = h('div.tabs.auth-tabs');

  function renderTabs() {
    tabsEl.replaceChildren(
      tabBtn('login', t('login.title')),
      tabBtn('register', t('login.registerTitle'))
    );
  }

  function tabBtn(id, label) {
    return h('button.tab', {
      type: 'button',
      class: `tab${mode === id ? ' is-active' : ''}`,
      'aria-selected': mode === id ? 'true' : 'false',
      text: label,
      onClick: () => {
        if (mode === id) return;
        // Смена через роутер — URL и состояние согласованы
        navigate(id === 'register' ? '#/register' : '#/login', { replace: true });
      },
    });
  }

  function showError(message) {
    errorBox.textContent = message;
    errorBox.classList.add('is-visible');
  }
  function hideError() {
    errorBox.classList.remove('is-visible');
    errorBox.textContent = '';
  }

  function renderForm() {
    formHost.replaceChildren();
    hideError();

    if (mode === 'login') {
      const loginF = field({ label: t('login.login'), name: 'login', autocomplete: 'username', required: true, placeholder: 'alice' });
      const passF = field({ label: t('login.password'), type: 'password', name: 'password', autocomplete: 'current-password', required: true });
      const submit = h('button.btn.btn-primary.btn-lg.btn-block', { type: 'submit', text: t('login.submitLogin') });

      const form = h('form', { novalidate: true },
        loginF, passF, errorBox, submit
      );
      form.addEventListener('submit', async (e) => {
        e.preventDefault();
        hideError();
        const login = loginF.control.value.trim();
        const password = passF.control.value;
        loginF.setError('');
        passF.setError('');

        if (login.length < 3) { loginF.setError(t('login.loginMin')); return; }
        if (password.length < 6) { passF.setError(t('login.passMin')); return; }

        submit.disabled = true;
        submit.textContent = t('login.submitLoginBusy');
        try {
          const session = await request('/auth/login', {
            method: 'POST',
            body: { login, password },
            auth: false,
          });
          setAuth(session.user, session);
          toast(t('login.welcome', { name: session.user.display_name || session.user.login }), 'success');
          navigate('#/', { replace: true });
        } catch (err) {
          const msg = err instanceof ApiError ? err.message : t('login.loginFailed');
          showError(msg);
          submit.disabled = false;
          submit.textContent = t('login.submitLogin');
        }
      });
      formHost.append(form, h('div.auth-switch', null,
        t('login.noAccount'),
        h('button', { type: 'button', text: t('action.register'), onClick: () => navigate('#/register') })
      ));
    } else {
      const loginF = field({
        label: t('login.login'), name: 'login', autocomplete: 'username', required: true,
        placeholder: 'alice', hint: t('login.loginHint'),
      });
      const nameF = field({ label: t('login.displayName'), name: 'display_name', autocomplete: 'name', placeholder: 'Alice' });
      const passF = field({
        label: t('login.password'), type: 'password', name: 'password',
        autocomplete: 'new-password', required: true, hint: t('login.passHint'),
      });
      const pass2F = field({ label: t('login.password2'), type: 'password', name: 'password2', autocomplete: 'new-password', required: true });
      const submit = h('button.btn.btn-primary.btn-lg.btn-block', { type: 'submit', text: t('action.createAccount') });

      const form = h('form', { novalidate: true },
        loginF, nameF, passF, pass2F, errorBox, submit
      );
      form.addEventListener('submit', async (e) => {
        e.preventDefault();
        hideError();
        const login = loginF.control.value.trim();
        const display_name = nameF.control.value.trim();
        const password = passF.control.value;
        const password2 = pass2F.control.value;
        [loginF, passF, pass2F].forEach((f) => f.setError(''));

        if (login.length < 3) { loginF.setError(t('login.loginMin')); return; }
        if (password.length < 6) { passF.setError(t('login.passMin')); return; }
        if (password !== password2) { pass2F.setError(t('login.passMismatch')); return; }

        submit.disabled = true;
        submit.textContent = t('login.submitRegisterBusy');
        try {
          await request('/auth/register', {
            method: 'POST',
            body: { login, password, ...(display_name ? { display_name } : {}) },
            auth: false,
          });
          // Регистрация вернула 201 User → автологин
          const session = await request('/auth/login', {
            method: 'POST',
            body: { login, password },
            auth: false,
          });
          setAuth(session.user, session);
          toast(t('login.registered'), 'success');
          navigate('#/', { replace: true });
        } catch (err) {
          const msg = err instanceof ApiError ? err.message : t('login.registerFailed');
          showError(msg);
          submit.disabled = false;
          submit.textContent = t('action.createAccount');
        }
      });
      formHost.append(form, h('div.auth-switch', null,
        t('login.hasAccount'),
        h('button', { type: 'button', text: t('action.login'), onClick: () => navigate('#/login') })
      ));
    }
  }

  // Язык: RU | EN — сразу на login/register, без входа.
  // Ререндер делает main.js на locale-changed (bare-режим) + здесь подстраховка title.
  const switcher = langSwitch(() => {
    const title = t(mode === 'register' ? 'login.registerTitle' : 'login.title');
    document.title = `${title} — ${t('app.brand')}`;
  });

  const card = h('div.auth-card.card',
    switcher,
    h('div.auth-brand', null,
      h('span.brand-mark', null,
        h('img.brand-mark-img', {
          src: '/static/team-logo-mark.png',
          alt: '',
          decoding: 'async',
          'aria-hidden': 'true',
        })
      ),
      h('div.auth-brand-text', null,
        h('h1', { text: t('app.brand') }),
        h('p', { text: t('app.tagline') })
      )
    ),
    tabsEl,
    formHost
  );

  root.replaceChildren(card);
  renderTabs();
  renderForm();
}
