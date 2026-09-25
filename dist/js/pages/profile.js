// Профиль: логин и имя, пароль, аватар — отдельная страница настроек.
//
// Все три операции трогают только учётную запись вызывающего:
// PATCH /profile, PUT /profile/password, PUT|DELETE /profile/avatar.

import { h } from '../core/dom.js';
import { field, toast, sectionCard, confirmModal } from '../core/components.js';
import { request, ApiError } from '../core/api.js';
import * as store from '../core/store.js';
import { t } from '../core/i18n.js';
import { invalidateAvatar, fetchAvatarUrl } from '../core/avatars.js';

/** Лимит аватара — зеркалит MAX_AVATAR_BYTES на сервере (1 МиB). */
const AVATAR_MAX_BYTES = 1024 * 1024;
const AVATAR_TYPES = ['image/png', 'image/jpeg', 'image/jpg', 'image/webp', 'image/gif'];

/** Сообщает shell'у, что профиль изменился (перерисовать шапку/сайдбар). */
function notifyUserUpdated() {
  window.dispatchEvent(new CustomEvent('user-updated'));
}

function errorMessage(err, fallback) {
  return err instanceof ApiError ? err.message : fallback;
}

export function renderPage(root) {
  // Guard от гонки: не дорисовываем/не тостим, если ушли со страницы.
  const marker = h('span', { style: { display: 'none' }, 'data-page': 'profile' });
  const isCurrent = () => marker.isConnected;

  const pageHeader = h('div.page-header', null,
    h('div', null,
      h('h1', { text: t('profile.title') }),
      h('div.page-sub', { text: t('profile.sub') })
    ),
    h('div.page-actions', null,
      h('a.btn.btn-ghost.btn-sm', { href: '#/settings' }, t('nav.settings'))
    )
  );

  const accountHost = h('div');
  const passwordHost = h('div');
  const avatarHost = h('div');
  const errorBox = h('div.form-error', { role: 'alert' });

  root.replaceChildren(marker, pageHeader, errorBox, avatarHost, accountHost, passwordHost);

  // ── Загрузка свежего профиля ──
  accountHost.append(h('div.card.mt-4', null,
    h('div.card-body', null, h('div.small.muted', { text: t('profile.loading') }))
  ));
  passwordHost.append(h('div.card.mt-4', null,
    h('div.card-body', null, h('div.small.muted', { text: t('profile.loading') }))
  ));
  avatarHost.append(h('div.card.mt-4', null,
    h('div.card-body', null, h('div.small.muted', { text: t('profile.loading') }))
  ));

  request('/profile')
    .then((user) => {
      if (!isCurrent()) return;
      store.setUser(user);
      renderAvatarSection(user);
      renderAccountSection(user);
      renderPasswordSection();
    })
    .catch((err) => {
      if (!isCurrent()) return;
      const msg = errorMessage(err, t('profile.loadFail'));
      toast(msg, 'error');
      for (const host of [avatarHost, accountHost, passwordHost]) {
        host.replaceChildren(h('div.card.mt-4', null,
          h('div.card-body', null, h('div.small.muted', { text: msg }))
        ));
      }
    });

  // ── Аватар ──
  function renderAvatarSection(user) {
    let pendingFile = null;   // { file, url } — выбран, ещё не загружен
    let previewUrl = null;    // objectURL превью (после удаления сбрасываем)

    const previewBox = h('div.avatar.avatar-lg.profile-avatar-lg.avatar-initials', {
      title: user.display_name || user.login || '',
    });

    function setPreviewFromUser() {
      if (previewUrl) { URL.revokeObjectURL(previewUrl); previewUrl = null; }
      pendingFile = null;
      uploadBtn.disabled = true;
      previewBox.classList.add('avatar-initials');
      previewBox.classList.remove('has-image');
      previewBox.replaceChildren(initialsOf(user));
      if (user.has_avatar) {
        fetchAvatarUrl(user.id).then((url) => {
          if (!url || !previewBox.isConnected || pendingFile) return;
          previewBox.replaceChildren(
            h('img.avatar-img', { src: url, alt: '', decoding: 'async' })
          );
          previewBox.classList.remove('avatar-initials');
          previewBox.classList.add('has-image');
        });
      }
    }

    const fileInput = h('input', {
      type: 'file',
      accept: AVATAR_TYPES.join(','),
      style: { display: 'none' },
      'aria-label': t('profile.avatarChoose'),
    });

    const pickBtn = h('button.btn.btn-secondary.btn-sm', {
      type: 'button',
      text: t('profile.avatarChoose'),
      onClick: () => fileInput.click(),
    });
    const uploadBtn = h('button.btn.btn-primary.btn-sm', {
      type: 'button',
      text: t('profile.avatarUpload'),
      disabled: true,
      onClick: async () => {
        if (!pendingFile) { toast(t('profile.avatarPickFirst'), 'warning'); return; }
        uploadBtn.disabled = true;
        uploadBtn.textContent = t('profile.avatarUploading');
        try {
          const fd = new FormData();
          fd.append('file', pendingFile.file, pendingFile.file.name);
          const updated = await request('/profile/avatar', { method: 'PUT', body: fd });
          if (!isCurrent()) return;
          store.setUser(updated);
          invalidateAvatar(updated.id);
          user = updated;
          removeBtn.hidden = !user.has_avatar;
          setPreviewFromUser();
          toast(t('profile.avatarUploaded'), 'success');
          notifyUserUpdated();
        } catch (err) {
          toast(errorMessage(err, t('profile.avatarUploadFail')), 'error');
          uploadBtn.disabled = false;
        } finally {
          uploadBtn.textContent = t('profile.avatarUpload');
          if (pendingFile && uploadBtn.isConnected) uploadBtn.disabled = false;
        }
      },
    });
    const removeBtn = h('button.btn.btn-danger.btn-sm', {
      type: 'button',
      text: t('profile.avatarRemove'),
      hidden: !user.has_avatar,
      onClick: async () => {
        const ok = await confirmModal({
          title: t('profile.avatarRemove'),
          message: t('profile.avatarRemoveConfirm'),
          confirmText: t('profile.avatarRemove'),
          danger: true,
        });
        if (!ok) return;
        removeBtn.disabled = true;
        try {
          const updated = await request('/profile/avatar', { method: 'DELETE' });
          if (!isCurrent()) return;
          store.setUser(updated);
          invalidateAvatar(updated.id);
          user = updated;
          removeBtn.hidden = !user.has_avatar;
          setPreviewFromUser();
          toast(t('profile.avatarRemoved'), 'success');
          notifyUserUpdated();
        } catch (err) {
          toast(errorMessage(err, t('profile.avatarRemoveFail')), 'error');
        } finally {
          removeBtn.disabled = false;
        }
      },
    });

    fileInput.addEventListener('change', () => {
      const file = fileInput.files && fileInput.files[0];
      fileInput.value = '';
      if (!file) return;
      if (!AVATAR_TYPES.includes(file.type)) {
        toast(t('profile.avatarBadType'), 'error');
        return;
      }
      if (file.size > AVATAR_MAX_BYTES) {
        toast(t('profile.avatarTooBig'), 'error');
        return;
      }
      if (previewUrl) URL.revokeObjectURL(previewUrl);
      pendingFile = { file };
      previewUrl = URL.createObjectURL(file);
      previewBox.replaceChildren(h('img.avatar-img', { src: previewUrl, alt: '' }));
      previewBox.classList.remove('avatar-initials');
      previewBox.classList.add('has-image');
      removeBtn.hidden = true; // в превью ещё не загружен — удалять нечего
      uploadBtn.disabled = false;
    });

    setPreviewFromUser();

    avatarHost.replaceChildren(sectionCard(
      t('profile.avatar'),
      t('profile.avatarDesc'),
      h('div.profile-avatar-row', null,
        previewBox,
        h('div.profile-avatar-actions', null, pickBtn, uploadBtn, removeBtn, fileInput)
      )
    ));
  }

  // ── Логин и имя ──
  function renderAccountSection(user) {
    const loginF = field({
      label: t('profile.login'),
      value: user.login || '',
      autocomplete: 'username',
      required: true,
      hint: t('profile.loginHint'),
    });
    const nameF = field({
      label: t('profile.displayName'),
      value: user.display_name || '',
      placeholder: t('profile.displayNamePlaceholder'),
      autocomplete: 'name',
      hint: t('profile.displayNameHint'),
    });

    const saveBtn = h('button.btn.btn-primary.btn-sm', {
      type: 'submit',
      text: t('profile.save'),
    });
    const form = h('form', { novalidate: true }, loginF, nameF, saveBtn);

    form.addEventListener('submit', async (e) => {
      e.preventDefault();
      loginF.setError('');
      const login = loginF.control.value.trim();
      const display_name = nameF.control.value.trim();

      if (login.length < 3) { loginF.setError(t('login.loginMin')); return; }

      saveBtn.disabled = true;
      saveBtn.textContent = t('profile.saving');
      try {
        const updated = await request('/profile', {
          method: 'PATCH',
          body: { login, display_name: display_name || null },
        });
        if (!isCurrent()) return;
        store.setUser(updated);
        toast(t('profile.saved'), 'success');
        notifyUserUpdated();
      } catch (err) {
        loginF.setError(errorMessage(err, t('profile.saveFail')));
        toast(errorMessage(err, t('profile.saveFail')), 'error');
      } finally {
        saveBtn.disabled = false;
        saveBtn.textContent = t('profile.save');
      }
    });

    accountHost.replaceChildren(sectionCard(
      t('profile.account'),
      t('profile.accountDesc'),
      form
    ));
  }

  // ── Пароль ──
  function renderPasswordSection() {
    const curF = field({
      label: t('profile.currentPassword'),
      type: 'password',
      autocomplete: 'current-password',
      required: true,
    });
    const newF = field({
      label: t('profile.newPassword'),
      type: 'password',
      autocomplete: 'new-password',
      required: true,
      hint: t('login.passHint'),
    });
    const repF = field({
      label: t('profile.confirmPassword'),
      type: 'password',
      autocomplete: 'new-password',
      required: true,
    });

    const saveBtn = h('button.btn.btn-primary.btn-sm', {
      type: 'submit',
      text: t('profile.changePassword'),
    });
    const form = h('form', { novalidate: true }, curF, newF, repF, saveBtn);

    form.addEventListener('submit', async (e) => {
      e.preventDefault();
      [curF, newF, repF].forEach((f) => f.setError(''));
      const current_password = curF.control.value;
      const new_password = newF.control.value;
      const repeat = repF.control.value;

      if (new_password.length < 6) { newF.setError(t('login.passMin')); return; }
      if (new_password !== repeat) { repF.setError(t('login.passMismatch')); return; }

      saveBtn.disabled = true;
      saveBtn.textContent = t('profile.changingPassword');
      try {
        await request('/profile/password', {
          method: 'PUT',
          body: { current_password, new_password },
        });
        if (!isCurrent()) return;
        toast(t('profile.passwordChanged'), 'success');
        curF.control.value = '';
        newF.control.value = '';
        repF.control.value = '';
      } catch (err) {
        const msg = errorMessage(err, t('profile.passwordFail'));
        curF.setError(msg);
        toast(msg, 'error');
      } finally {
        saveBtn.disabled = false;
        saveBtn.textContent = t('profile.changePassword');
      }
    });

    passwordHost.replaceChildren(sectionCard(
      t('profile.password'),
      t('profile.passwordDesc'),
      form
    ));
  }
}

/** Инициалы для блока-заглушки (пока нет картинки). */
function initialsOf(u) {
  const name = (u && (u.display_name || u.login)) || '?';
  const text = name
    .split(/\s+/)
    .slice(0, 2)
    .map((p) => p[0])
    .join('')
    .toUpperCase();
  return document.createTextNode(text);
}
