// Страница входа/регистрации — полностью рабочая

import { h } from '../core/dom.js';
import { field, toast } from '../core/components.js';
import { request, ApiError } from '../core/api.js';
import { setAuth } from '../core/store.js';
import { navigate } from '../core/router.js';

export function renderPage(root, params = {}) {
  let mode = params.mode === 'register' ? 'register' : 'login';

  const errorBox = h('div.form-error', { role: 'alert' });
  const formHost = h('div.auth-form');

  const tabsEl = h('div.tabs.auth-tabs');

  function renderTabs() {
    tabsEl.replaceChildren(
      tabBtn('login', 'Вход'),
      tabBtn('register', 'Регистрация')
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
      const loginF = field({ label: 'Логин', name: 'login', autocomplete: 'username', required: true, placeholder: 'alice' });
      const passF = field({ label: 'Пароль', type: 'password', name: 'password', autocomplete: 'current-password', required: true });
      const submit = h('button.btn.btn-primary.btn-lg.btn-block', { type: 'submit', text: 'Войти' });

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

        if (login.length < 3) { loginF.setError('Логин — минимум 3 символа'); return; }
        if (password.length < 6) { passF.setError('Пароль — минимум 6 символов'); return; }

        submit.disabled = true;
        submit.textContent = 'Входим…';
        try {
          const session = await request('/auth/login', {
            method: 'POST',
            body: { login, password },
            auth: false,
          });
          setAuth(session.user, session);
          toast(`Добро пожаловать, ${session.user.display_name || session.user.login}!`, 'success');
          navigate('#/', { replace: true });
        } catch (err) {
          const msg = err instanceof ApiError ? err.message : 'Не удалось войти';
          showError(msg);
          submit.disabled = false;
          submit.textContent = 'Войти';
        }
      });
      formHost.append(form, h('div.auth-switch', null,
        'Нет учётной записи? ',
        h('button', { type: 'button', text: 'Зарегистрироваться', onClick: () => navigate('#/register') })
      ));
    } else {
      const loginF = field({
        label: 'Логин', name: 'login', autocomplete: 'username', required: true,
        placeholder: 'alice', hint: 'Латиница, цифры, _ - . ; минимум 3 символа',
      });
      const nameF = field({ label: 'Имя (необязательно)', name: 'display_name', autocomplete: 'name', placeholder: 'Алиса' });
      const passF = field({
        label: 'Пароль', type: 'password', name: 'password',
        autocomplete: 'new-password', required: true, hint: 'Минимум 6 символов',
      });
      const pass2F = field({ label: 'Повторите пароль', type: 'password', name: 'password2', autocomplete: 'new-password', required: true });
      const submit = h('button.btn.btn-primary.btn-lg.btn-block', { type: 'submit', text: 'Создать аккаунт' });

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

        if (login.length < 3) { loginF.setError('Логин — минимум 3 символа'); return; }
        if (password.length < 6) { passF.setError('Пароль — минимум 6 символов'); return; }
        if (password !== password2) { pass2F.setError('Пароли не совпадают'); return; }

        submit.disabled = true;
        submit.textContent = 'Создаём…';
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
          toast('Аккаунт создан — вы вошли', 'success');
          navigate('#/', { replace: true });
        } catch (err) {
          const msg = err instanceof ApiError ? err.message : 'Не удалось зарегистрироваться';
          showError(msg);
          submit.disabled = false;
          submit.textContent = 'Создать аккаунт';
        }
      });
      formHost.append(form, h('div.auth-switch', null,
        'Уже есть аккаунт? ',
        h('button', { type: 'button', text: 'Войти', onClick: () => navigate('#/login') })
      ));
    }
  }

  const card = h('div.auth-card.card',
    h('div.auth-brand', null,
      h('span.brand-mark', { text: 'А' }),
      h('div.auth-brand-text', null,
        h('h1', { text: 'Арена Переговоров' }),
        h('p', { text: 'Тренажёр переговоров с ИИ' })
      )
    ),
    tabsEl,
    formHost
  );

  root.replaceChildren(card);
  renderTabs();
  renderForm();
}
