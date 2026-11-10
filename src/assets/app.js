'use strict';

const $ = (selector, root = document) => root.querySelector(selector);
const $$ = (selector, root = document) => Array.from(root.querySelectorAll(selector));

const WEEKDAYS = ['周日', '周一', '周二', '周三', '周四', '周五', '周六'];

const state = {
  from: '',
  to: '',
  preset: 'today',
  page: 1,
  pageSize: 20,
  q: '',
  bundle: '',
  heatDays: 14,
  total: 0,
  loading: false,
  dash: null,
};

init();

function init() {
  applyPreset(initialRange(), false);
  bindEvents();
  setDateLimits();
  renderSkeleton();
  loadAll();
  loadStatus();

  window.setInterval(() => {
    if (document.hidden || state.loading) return;
    if (state.preset !== 'custom') applyPreset(state.preset, false);
    loadAll({ silent: true });
  }, 10000);

  window.setInterval(() => {
    if (!document.hidden) loadStatus();
  }, 5000);
}

/* ---------- events ---------- */

function bindEvents() {
  $('#rangeSeg').addEventListener('click', (event) => {
    const button = event.target.closest('button[data-range]');
    if (!button) return;
    applyPreset(button.dataset.range);
  });

  $('#heatSeg').addEventListener('click', (event) => {
    const button = event.target.closest('button[data-days]');
    if (!button) return;
    state.heatDays = Number(button.dataset.days) || 14;
    $$('#heatSeg button').forEach((btn) => btn.classList.toggle('active', btn === button));
    loadHeatmap();
  });

  $('#fromDate').addEventListener('change', onDateChange);
  $('#toDate').addEventListener('change', onDateChange);

  $('#searchInput').addEventListener(
    'input',
    debounce(() => {
      state.q = $('#searchInput').value;
      state.page = 1;
      loadSessions();
    }, 300),
  );

  $('#appFilter').addEventListener('change', () => {
    state.bundle = $('#appFilter').value;
    state.page = 1;
    syncFilterChip();
    if (state.dash) renderApps(state.dash);
    loadSessions();
  });

  $('#resetFilter').addEventListener('click', clearBundleFilter);

  $('#prevPage').addEventListener('click', () => {
    if (state.page > 1) {
      state.page -= 1;
      loadSessions();
    }
  });

  $('#nextPage').addEventListener('click', () => {
    const pages = Math.max(1, Math.ceil(state.total / state.pageSize));
    if (state.page < pages) {
      state.page += 1;
      loadSessions();
    }
  });

  $('#refreshBtn').addEventListener('click', () => {
    loadAll();
    loadStatus();
  });

  $('#exportBtn').addEventListener('click', exportCsv);

  $('#dismissBanner').addEventListener('click', () => {
    sessionStorage.setItem('ax-banner-dismissed', '1');
    $('#axBanner').hidden = true;
  });

  $('#appList').addEventListener('click', onAppClick);
}

function onDateChange(event) {
  const from = $('#fromDate').value;
  const to = $('#toDate').value;
  if (!from || !to) return;

  state.from = from;
  state.to = to;
  if (state.from > state.to) {
    if (event.target.id === 'fromDate') {
      state.to = state.from;
    } else {
      state.from = state.to;
    }
  }
  state.preset = 'custom';
  state.page = 1;
  syncDateInputs();
  markPreset(null);
  loadAll();
}

function onAppClick(event) {
  const row = event.target.closest('.app-row');
  if (!row) return;
  const key = row.dataset.key;
  state.bundle = state.bundle === key ? '' : key;
  state.page = 1;
  $('#appFilter').value = state.bundle;
  syncFilterChip();
  if (state.dash) renderApps(state.dash);
  loadSessions();
}

function clearBundleFilter() {
  state.bundle = '';
  $('#appFilter').value = '';
  state.page = 1;
  syncFilterChip();
  if (state.dash) renderApps(state.dash);
  loadSessions();
}

/* ---------- data loading ---------- */

async function loadAll(options = {}) {
  if (state.loading) return;
  state.loading = true;
  if (!options.silent) $('#refreshBtn').classList.add('busy');

  try {
    await loadDashboard();
    await Promise.all([loadSessions(), loadHeatmap()]);
    $('#updatedAt').textContent = `最后更新 ${new Date().toLocaleTimeString('zh-CN', { hour12: false })}`;
  } catch (error) {
    console.error(error);
  } finally {
    state.loading = false;
    $('#refreshBtn').classList.remove('busy');
  }
}

async function loadDashboard() {
  const data = await fetchJSON(
    `/api/dashboard?from=${encodeURIComponent(state.from)}&to=${encodeURIComponent(state.to)}`,
  );
  state.dash = data;
  state.apps = data.apps;

  if (state.bundle && !data.apps.some((app) => appKey(app) === state.bundle)) {
    state.bundle = '';
  }

  renderStats(data);
  renderApps(data);
  renderHours(data);
  renderAppFilter(data.apps);
  syncFilterChip();
}

async function loadSessions() {
  const params = new URLSearchParams({
    from: state.from,
    to: state.to,
    page: String(state.page),
    page_size: String(state.pageSize),
  });
  if (state.q) params.set('q', state.q);
  if (state.bundle) params.set('bundle', state.bundle);

  const data = await fetchJSON(`/api/sessions?${params.toString()}`);
  state.total = data.total;

  const pages = Math.max(1, Math.ceil(data.total / data.page_size));
  if (state.page > pages) {
    state.page = pages;
    return loadSessions();
  }
  renderSessions(data);
}

async function loadHeatmap() {
  const data = await fetchJSON(`/api/heatmap?days=${state.heatDays}`);
  renderHeatmap(data);
}

async function loadStatus() {
  try {
    const status = await fetchJSON('/api/status');
    const dismissed = sessionStorage.getItem('ax-banner-dismissed') === '1';
    $('#axBanner').hidden = status.ax_trusted || !status.track_enabled || dismissed;
    renderAxBanner(status);
  } catch (error) {
    console.error(error);
  }
}

function renderAxBanner(status) {
  const text = $('#axBannerText');
  if (!text) return;
  const tail = '应用名与时长仍会正常记录。';
  if (status.launch_kind === 'terminal') {
    const name = status.terminal_name || '终端';
    text.textContent = `请在「系统设置 → 隐私与安全性 → 辅助功能」中允许 ${name}，然后完全退出并重开 ${name}。${tail}`;
    return;
  }
  const target = status.exe_path || 'oxidetime';
  if (status.launch_kind === 'brew') {
    text.textContent = `OxideTime 正通过 Homebrew 服务运行。请在「系统设置 → 隐私与安全性 → 辅助功能」中点击「+」，添加 ${target}，然后执行 brew services restart oxidetime。${tail}`;
    return;
  }
  text.textContent = `请在「系统设置 → 隐私与安全性 → 辅助功能」中点击「+」，添加 ${target}，然后重启 OxideTime。${tail}`;
}

async function fetchJSON(url) {
  const response = await fetch(url, { cache: 'no-store' });
  if (!response.ok) throw new Error(`${response.status} ${response.statusText} · ${url}`);
  return response.json();
}

function exportCsv() {
  const params = new URLSearchParams({ from: state.from, to: state.to });
  if (state.q) params.set('q', state.q);
  if (state.bundle) params.set('bundle', state.bundle);
  window.location.href = `/api/export.csv?${params.toString()}`;
}

/* ---------- renderers ---------- */

function renderSkeleton() {
  $('#appList').innerHTML = Array.from(
    { length: 5 },
    () => '<div class="skeleton-row"></div>',
  ).join('');
}

function renderStats(data) {
  const summary = data.summary;
  const total = summary.total_ms || 0;
  const days = rangeDays();

  setText('#statTotal', total ? fmtDuration(total) : '0 秒');
  if (total && days === 1 && summary.first_at) {
    setText('#statTotalSub', `${fmtHHMM(summary.first_at)} – ${fmtHHMM(summary.last_at)} 有记录`);
  } else if (total) {
    setText('#statTotalSub', `${days} 天区间 · 日均 ${fmtDuration(Math.round(total / days))}`);
  } else {
    setText('#statTotalSub', '所选区间暂无数据');
  }

  setText('#statApps', String(summary.app_count));
  setText(
    '#statAppsSub',
    summary.top_app
      ? `最常用 ${summary.top_app.app_name} · ${fmtDuration(summary.top_app.total_ms)}`
      : '—',
  );

  setText('#statSessions', summary.session_count.toLocaleString('zh-CN'));
  setText(
    '#statSessionsSub',
    summary.session_count ? `平均每次 ${fmtDuration(summary.avg_session_ms)}` : '—',
  );

  if (summary.busiest_hour && total) {
    const hour = summary.busiest_hour.hour;
    const share = Math.round((summary.busiest_hour.total_ms / total) * 100);
    setText('#statPeak', `${pad(hour)}:00 – ${pad(hour + 1)}:00`);
    setText(
      '#statPeakSub',
      `占${days === 1 ? '全天' : '区间'} ${share}% · ${fmtDuration(summary.busiest_hour.total_ms)}`,
    );
  } else {
    setText('#statPeak', '—');
    setText('#statPeakSub', '—');
  }
}

function renderApps(data) {
  const list = $('#appList');
  const apps = data.apps;
  const total = data.summary.total_ms || 1;

  setText(
    '#appSub',
    apps.length
      ? `${apps.length} 个应用 · 共 ${fmtDuration(data.summary.total_ms)}`
      : '所选区间暂无数据',
  );

  if (!apps.length) {
    list.innerHTML = '<div class="app-empty">暂无应用记录</div>';
    return;
  }

  const max = apps[0].total_ms || 1;
  list.innerHTML = apps
    .map((app) => {
      const key = appKey(app);
      const pct = (app.total_ms / total) * 100;
      const width = Math.max(2, (app.total_ms / max) * 100);
      const selected = key === state.bundle ? ' selected' : '';
      return `<button type="button" class="app-row${selected}" data-key="${esc(key)}" title="${esc(app.app_name)}${app.bundle_id ? ' · ' + esc(app.bundle_id) : ''}">
        ${avatarHTML(app)}
        <span class="app-meta">
          <span class="app-name">${esc(app.app_name)}</span>
          <span class="app-sessions">${app.sessions} 次 · ${relativeTime(app.last_used)}</span>
        </span>
        <span class="app-bar"><i style="width:${width.toFixed(1)}%"></i></span>
        <span class="app-time">${fmtDuration(app.total_ms)}</span>
        <span class="app-pct">${formatPct(pct)}</span>
      </button>`;
    })
    .join('');
}

function renderHours(data) {
  const totals = new Array(24).fill(0);
  data.hourly.forEach((row) => {
    if (row.hour >= 0 && row.hour < 24) totals[row.hour] += row.total_ms;
  });

  const hasData = data.summary.total_ms > 0;
  const max = Math.max(...totals, 1);
  const peak = totals.indexOf(Math.max(...totals));

  setText('#hourPeak', hasData ? `峰值 ${pad(peak)}:00` : '—');

  $('#hourChart').innerHTML = totals
    .map((value, hour) => {
      const height = hasData && value ? 3 + (value / max) * 84 : 1.2;
      const classes = ['hour-col', value > 0 && hour === peak ? 'peak' : '']
        .filter(Boolean)
        .join(' ');
      const title = `${pad(hour)}:00 – ${pad(hour + 1)}:00 · ${fmtDuration(value)}`;
      return `<div class="${classes}" title="${title}">
        <div class="hour-bar${value === 0 ? ' zero' : ''}" style="height:${height.toFixed(1)}%"></div>
        <span class="hour-label">${hour % 3 === 0 ? pad(hour) : ''}</span>
      </div>`;
    })
    .join('');
}

function renderHeatmap(data) {
  const container = $('#heatmap');
  if (!data.days.length) {
    container.innerHTML = '';
    return;
  }

  const maxHour = Math.max(1, ...data.days.flatMap((day) => day.hours));
  const hours = Array.from({ length: 24 }, (_, hour) => `<span>${hour % 3 === 0 ? pad(hour) : ''}</span>`).join('');
  const head = `<div class="heat-row heat-head"><span></span>${hours}</div>`;

  const body = data.days
    .map((day) => {
      const [year, month, dayOfMonth] = day.date.split('-').map(Number);
      const weekday = WEEKDAYS[new Date(year, month - 1, dayOfMonth).getDay()];
      const cells = day.hours
        .map((value, hour) => {
          const level = heatLevel(value, maxHour);
          const title = `${day.date} ${pad(hour)}:00 · ${fmtDuration(value)}`;
          return `<span class="heat-cell lv${level}" title="${title}"></span>`;
        })
        .join('');
      return `<div class="heat-row"><span class="heat-day">${pad(month)}-${pad(dayOfMonth)}<i>${weekday}</i></span>${cells}</div>`;
    })
    .join('');

  const first = data.days[0].date;
  const last = data.days[data.days.length - 1].date;
  const foot = `<div class="heat-foot">
      <span>${first} → ${last}</span>
      <span class="legend">少 <i class="lv1"></i><i class="lv2"></i><i class="lv3"></i><i class="lv4"></i> 多</span>
    </div>`;

  container.innerHTML = head + body + foot;
}

function renderSessions(data) {
  const tbody = $('#sessionRows');
  const empty = $('#emptyState');
  const multiDay = state.from !== state.to;

  if (!data.items.length) {
    tbody.innerHTML = '';
    empty.hidden = false;
  } else {
    empty.hidden = true;
    tbody.innerHTML = data.items
      .map(
        (row) => `<tr>
          <td>
            <div class="cell-app">
              ${avatarHTML({ bundle_id: row.bundle_id, app_name: row.app_name }, 'small')}
              <div class="cell-text">
                <span class="cell-name">${esc(row.app_name)}</span>
                <span class="cell-title">${row.window_title ? esc(row.window_title) : '<em>无窗口标题</em>'}</span>
              </div>
            </div>
          </td>
          <td class="num time">${fmtStamp(row.started_at, multiDay)}</td>
          <td class="num time">${fmtStamp(row.ended_at, multiDay)}</td>
          <td class="num dur">${fmtDuration(row.duration_ms)}</td>
        </tr>`,
      )
      .join('');
  }

  const pages = Math.max(1, Math.ceil(data.total / data.page_size));
  setText('#sessionsSub', data.total ? `${data.total} 条会话` : '所选区间暂无记录');
  setText(
    '#pageInfo',
    data.total ? `第 ${data.page} / ${pages} 页 · 共 ${data.total} 条` : '暂无记录',
  );
  $('#prevPage').disabled = data.page <= 1;
  $('#nextPage').disabled = data.page >= pages;
}

function renderAppFilter(apps) {
  const select = $('#appFilter');
  const previous = state.bundle;
  select.innerHTML =
    '<option value="">全部应用</option>' +
    apps
      .map(
        (app) =>
          `<option value="${esc(appKey(app))}">${esc(app.app_name)} · ${fmtDuration(app.total_ms)}</option>`,
      )
      .join('');
  select.value = previous;
  if (select.value !== previous) {
    select.value = '';
    state.bundle = '';
  }
  syncFilterChip();
}

function syncFilterChip() {
  $('#resetFilter').hidden = !state.bundle;
}

/* ---------- date range ---------- */

function initialRange() {
  const range = new URLSearchParams(window.location.search).get('range');
  return ['today', 'yesterday', '7d', '30d'].includes(range) ? range : 'today';
}

function applyPreset(preset, reload = true) {
  const today = todayStr();
  let from = today;
  let to = today;

  if (preset === 'yesterday') {
    from = addDays(today, -1);
    to = from;
  } else if (preset === '7d') {
    from = addDays(today, -6);
  } else if (preset === '30d') {
    from = addDays(today, -29);
  }

  state.from = from;
  state.to = to;
  state.preset = preset;
  state.page = 1;
  syncDateInputs();
  markPreset(preset);
  if (reload) loadAll();
}

function syncDateInputs() {
  $('#fromDate').value = state.from;
  $('#toDate').value = state.to;
}

function markPreset(preset) {
  $$('#rangeSeg button').forEach((button) => {
    button.classList.toggle('active', button.dataset.range === preset);
  });
}

function setDateLimits() {
  const today = todayStr();
  $('#fromDate').max = today;
  $('#toDate').max = today;
}

function rangeDays() {
  return Math.round((parseDateStr(state.to) - parseDateStr(state.from)) / 86400000) + 1;
}

function todayStr() {
  return fmtDate(new Date());
}

function fmtDate(date) {
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

function parseDateStr(value) {
  const [year, month, day] = value.split('-').map(Number);
  return new Date(year, month - 1, day);
}

function addDays(value, delta) {
  const date = parseDateStr(value);
  date.setDate(date.getDate() + delta);
  return fmtDate(date);
}

/* ---------- formatting ---------- */

function pad(value) {
  return String(value).padStart(2, '0');
}

function fmtDuration(ms) {
  if (!ms || ms < 1000) return '0 秒';
  const totalSeconds = Math.round(ms / 1000);
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;

  if (hours > 0) return minutes > 0 ? `${hours} 小时 ${minutes} 分` : `${hours} 小时`;
  if (minutes > 0) return seconds > 0 ? `${minutes} 分 ${seconds} 秒` : `${minutes} 分`;
  return `${seconds} 秒`;
}

function fmtStamp(ms, withDate) {
  const date = new Date(ms);
  const time = `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
  return withDate ? `${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${time}` : time;
}

function fmtHHMM(ms) {
  const date = new Date(ms);
  return `${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

function relativeTime(ms) {
  const diff = Date.now() - ms;
  if (diff < 60_000) return '刚刚';
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)} 分钟前`;
  const date = new Date(ms);
  const now = new Date();
  if (date.toDateString() === now.toDateString()) return `今天 ${fmtHHMM(ms)}`;
  return `${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${fmtHHMM(ms)}`;
}

function formatPct(value) {
  if (value >= 99.5) return '100%';
  return value >= 10 ? `${Math.round(value)}%` : `${value.toFixed(1)}%`;
}

function heatLevel(value, max) {
  if (!value) return 0;
  const ratio = value / max;
  if (ratio >= 0.66) return 4;
  if (ratio >= 0.33) return 3;
  if (ratio >= 0.12) return 2;
  return 1;
}

function appKey(app) {
  return app.bundle_id || app.app_name;
}

function avatarHTML(app, extra = '') {
  const key = appKey(app) || 'unknown';
  const src = `/api/icons/${encodeURIComponent(key)}?v=2`;
  return `<span class="avatar ${extra}"><img src="${src}" alt="" loading="lazy" onerror="this.remove()"></span>`;
}

function esc(value) {
  return String(value ?? '').replace(
    /[&<>"']/g,
    (char) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[char],
  );
}

function setText(selector, value) {
  const element = $(selector);
  if (element) element.textContent = value;
}

function debounce(fn, wait) {
  let timer = null;
  return (...args) => {
    window.clearTimeout(timer);
    timer = window.setTimeout(() => fn(...args), wait);
  };
}
