/* aoko client — interactive preview.
 * Builds a faux Windows desktop with a draggable "aoko client" window whose
 * contents are rendered from AOKO_PREVIEW_DATA. Pure client-side illusion.
 */
(function () {
  'use strict';

  var DATA = window.AOKO_PREVIEW_DATA;
  var launchBtn = document.getElementById('preview-open');
  var desktop = document.getElementById('aoko-desktop');
  if (!DATA || !launchBtn || !desktop) return;

  var reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  var mobileQuery = window.matchMedia('(max-width: 760px)');

  /* ---------- small DOM helpers ---------- */
  function el(tag, cls, attrs) {
    var n = document.createElement(tag);
    if (cls) n.className = cls;
    if (attrs) for (var k in attrs) {
      if (k === 'text') n.textContent = attrs[k];
      else n.setAttribute(k, attrs[k]);
    }
    return n;
  }
  function hexToRgb(hex) {
    hex = hex.replace('#', '');
    if (hex.length === 3) hex = hex.split('').map(function (c) { return c + c; }).join('');
    var n = parseInt(hex, 16);
    return { r: (n >> 16) & 255, g: (n >> 8) & 255, b: n & 255 };
  }
  function rgba(hex, a) {
    var c = hexToRgb(hex);
    return 'rgba(' + c.r + ',' + c.g + ',' + c.b + ',' + a + ')';
  }

  /* ---------- state ---------- */
  var built = false;
  var win, taskApp, clockEl, contentWrap, railWrap;
  var lastFocused = null;
  var savedScrollY = 0;
  var activeTab = DATA.tabs[0].id;
  var currentPalette = DATA.defaultPalette;
  var currentModStyle = DATA.defaultModuleStyle;
  var winRect = null; // remembered rect before maximize

  /* =====================================================================
   * BUILD — desktop shell + window chrome
   * ===================================================================== */
  function buildDesktop() {
    if (built) return;
    built = true;

    var wallpaper = el('div', 'aoko-wallpaper');
    wallpaper.setAttribute('aria-hidden', 'true');
    desktop.appendChild(wallpaper);

    // exit affordance (outside the illusion)
    var exit = el('button', 'aoko-exit', { type: 'button', 'aria-label': 'Close preview' });
    exit.innerHTML = '&#x2715; exit preview';
    exit.addEventListener('click', close);
    desktop.appendChild(exit);

    buildWindow();
    buildTaskbar();
    applyPalette(currentPalette);
  }

  function buildWindow() {
    win = el('div', 'aoko-window', { role: 'group', 'aria-label': 'aoko client window', tabindex: '-1' });

    // title bar
    var bar = el('div', 'aoko-titlebar');
    var titleWrap = el('div', 'aoko-title');
    var icon = el('span', 'aoko-title-icon', { 'aria-hidden': 'true' });
    icon.textContent = '\u25D0';
    var titleText = el('span', 'aoko-title-text', { text: 'aoko client' });
    titleWrap.appendChild(icon);
    titleWrap.appendChild(titleText);

    var controls = el('div', 'aoko-winbtns');
    var minBtn = el('button', 'aoko-winbtn min', { type: 'button', 'aria-label': 'Minimize', title: 'Minimize' });
    minBtn.innerHTML = '<svg viewBox="0 0 10 10" aria-hidden="true"><rect x="1" y="5" width="8" height="1"/></svg>';
    var maxBtn = el('button', 'aoko-winbtn max', { type: 'button', 'aria-label': 'Maximize', title: 'Maximize' });
    maxBtn.innerHTML = '<svg viewBox="0 0 10 10" aria-hidden="true"><rect x="1.2" y="1.2" width="7.6" height="7.6" fill="none" stroke="currentColor" stroke-width="1"/></svg>';
    var closeBtn = el('button', 'aoko-winbtn close', { type: 'button', 'aria-label': 'Close', title: 'Close' });
    closeBtn.innerHTML = '<svg viewBox="0 0 10 10" aria-hidden="true"><path d="M1 1 L9 9 M9 1 L1 9" stroke="currentColor" stroke-width="1.1"/></svg>';
    controls.appendChild(minBtn);
    controls.appendChild(maxBtn);
    controls.appendChild(closeBtn);

    bar.appendChild(titleWrap);
    bar.appendChild(controls);
    win.appendChild(bar);

    // body: rail + content
    var body = el('div', 'aoko-body');
    railWrap = el('div', 'aoko-rail', { role: 'tablist', 'aria-label': 'Module categories' });
    contentWrap = el('div', 'aoko-content');
    body.appendChild(railWrap);
    body.appendChild(contentWrap);
    win.appendChild(body);

    desktop.appendChild(win);

    renderRail();
    renderAllTabs();
    selectTab(activeTab);

    // window control wiring
    minBtn.addEventListener('click', minimize);
    maxBtn.addEventListener('click', toggleMaximize);
    closeBtn.addEventListener('click', close);
    bar.addEventListener('dblclick', function (e) {
      if (e.target.closest('.aoko-winbtn')) return;
      toggleMaximize();
    });

    enableDrag(bar);
  }

  function buildTaskbar() {
    var taskbar = el('div', 'aoko-taskbar');
    var start = el('button', 'aoko-start', { type: 'button', 'aria-label': 'Start' });
    start.innerHTML =
      '<span class="aoko-start-orb" aria-hidden="true">' +
      '<i></i><i></i><i></i><i></i></span>';
    start.addEventListener('click', function () { start.classList.toggle('active'); });

    taskApp = el('button', 'aoko-taskapp active', { type: 'button', 'aria-label': 'aoko client', title: 'aoko client' });
    taskApp.innerHTML = '<span class="aoko-taskapp-icon" aria-hidden="true">\u25D0</span><span>aoko client</span>';
    taskApp.addEventListener('click', function () {
      if (win.classList.contains('minimized')) restore();
      else minimize();
    });

    var tray = el('div', 'aoko-tray');
    clockEl = el('div', 'aoko-clock', { 'aria-hidden': 'true' });
    tray.appendChild(clockEl);

    taskbar.appendChild(start);
    taskbar.appendChild(taskApp);
    taskbar.appendChild(tray);
    desktop.appendChild(taskbar);

    tickClock();
    setInterval(tickClock, 1000);
  }

  function tickClock() {
    if (!clockEl) return;
    var d = new Date();
    var h = d.getHours(), m = d.getMinutes();
    var ampm = h >= 12 ? 'PM' : 'AM';
    var hh = h % 12; if (hh === 0) hh = 12;
    var time = hh + ':' + String(m).padStart(2, '0') + ' ' + ampm;
    var date = (d.getMonth() + 1) + '/' + d.getDate() + '/' + d.getFullYear();
    clockEl.innerHTML = '<span class="t">' + time + '</span><span class="d">' + date + '</span>';
  }

  /* =====================================================================
   * RENDER — tabs, cards, controls
   * ===================================================================== */
  function renderRail() {
    railWrap.innerHTML = '';
    DATA.tabs.forEach(function (tab) {
      var b = el('button', 'aoko-tab', {
        type: 'button', role: 'tab', 'data-tab': tab.id,
        id: 'aoko-tab-' + tab.id, 'aria-controls': 'aoko-panel-' + tab.id,
      });
      b.textContent = tab.label;
      b.addEventListener('click', function () { selectTab(tab.id); });
      railWrap.appendChild(b);
    });
  }

  function renderAllTabs() {
    contentWrap.innerHTML = '';
    DATA.tabs.forEach(function (tab) {
      var panel = el('div', 'aoko-panel', {
        role: 'tabpanel', id: 'aoko-panel-' + tab.id,
        'aria-labelledby': 'aoko-tab-' + tab.id,
      });
      var scroll = el('div', 'aoko-scroll');
      tab.cards.forEach(function (card) { scroll.appendChild(renderCard(card)); });
      panel.appendChild(scroll);
      contentWrap.appendChild(panel);
    });
  }

  function selectTab(id) {
    activeTab = id;
    railWrap.querySelectorAll('.aoko-tab').forEach(function (t) {
      var on = t.getAttribute('data-tab') === id;
      t.classList.toggle('active', on);
      t.setAttribute('aria-selected', on ? 'true' : 'false');
      t.setAttribute('tabindex', on ? '0' : '-1');
    });
    contentWrap.querySelectorAll('.aoko-panel').forEach(function (p) {
      p.classList.toggle('active', p.id === 'aoko-panel-' + id);
    });
  }

  function renderCard(card) {
    var wrap = el('div', 'aoko-card');
    var gated = []; // elements to enable/disable with the card's main control
    var mainState = { on: true, has: false };

    // header
    var head = el('div', 'aoko-card-head');
    var h = el('div', 'aoko-card-title', { text: card.title });
    head.appendChild(h);
    if (card.toggle) {
      mainState.has = true;
      mainState.on = !!card.toggle.on;
      var tg = makeToggle(card.toggle.on, card.title);
      if (card.toggle.main) {
        tg.addEventListener('aoko-change', function (e) {
          mainState.on = e.detail; updateGate(); updateStatus();
        });
      }
      head.appendChild(tg);
    }
    wrap.appendChild(head);

    // status sub-label (coral)
    var statusEl = null;
    if (card.status) {
      statusEl = el('div', 'aoko-status');
      wrap.appendChild(statusEl);
    }
    function updateStatus() {
      if (!statusEl) return;
      if (typeof card.status === 'string') statusEl.textContent = card.status;
      else statusEl.textContent = mainState.on ? card.status.armed : card.status.available;
    }

    // controls
    (card.controls || []).forEach(function (ctrl) {
      var node = renderControl(ctrl, mainState, gated);
      if (node) wrap.appendChild(node);
    });

    function updateGate() {
      var enabled = !mainState.has || mainState.on;
      gated.forEach(function (g) {
        g.classList.toggle('gated-off', !enabled);
        g.querySelectorAll('input, select, button').forEach(function (f) {
          f.disabled = !enabled;
        });
      });
    }

    // main switch-rows (e.g. "Enable Right Click") bubble aoko-main
    wrap.addEventListener('aoko-main', function (e) {
      mainState.on = e.detail;
      updateGate();
      updateStatus();
    });

    updateStatus();
    updateGate();
    return wrap;
  }

  function renderControl(ctrl, mainState, gated) {
    var node;
    switch (ctrl.type) {
      case 'slider':   node = makeSlider(ctrl); break;
      case 'checks':   node = makeChecks(ctrl); break;
      case 'check':    node = makeCheck(ctrl); break;
      case 'switch':   node = makeSwitchRow(ctrl, mainState); break;
      case 'select':   node = makeSelect(ctrl); break;
      case 'button':   node = makeButton(ctrl.label); break;
      case 'buttons':  node = makeButtons(ctrl.items); break;
      case 'note':     node = makeNote(ctrl.text, ctrl.warn); break;
      case 'keybinds': node = makeKeybinds(ctrl.items); break;
      case 'palettes': node = makePalettes(); break;
      case 'modstyles':node = makeModStyles(); break;
      default: node = null;
    }
    if (node && ctrl.gate) gated.push(node);
    return node;
  }

  /* ---------- control factories ---------- */
  function makeToggle(on, label) {
    var b = el('button', 'aoko-toggle' + (on ? ' on' : ''), {
      type: 'button', role: 'switch', 'aria-checked': on ? 'true' : 'false',
      'aria-label': label || 'toggle',
    });
    b.innerHTML = '<span class="aoko-toggle-knob"></span>';
    b.addEventListener('click', function () {
      var next = !b.classList.contains('on');
      b.classList.toggle('on', next);
      b.setAttribute('aria-checked', next ? 'true' : 'false');
      b.dispatchEvent(new CustomEvent('aoko-change', { detail: next, bubbles: false }));
    });
    return b;
  }

  function makeSlider(c) {
    var row = el('div', 'aoko-ctrl aoko-slider');
    var head = el('div', 'aoko-slider-head');
    var label = el('span', 'aoko-slider-label', { text: c.label });
    var val = el('span', 'aoko-slider-val');
    head.appendChild(label);
    head.appendChild(val);
    var input = el('input', 'aoko-range', {
      type: 'range', min: c.min, max: c.max, step: c.step,
      value: c.value, 'aria-label': c.label,
    });
    function fmt(v) { return Number(v).toFixed(c.decimals || 0); }
    function paint() {
      var pct = ((input.value - c.min) / (c.max - c.min)) * 100;
      input.style.setProperty('--fill', pct + '%');
      val.textContent = fmt(input.value);
    }
    input.addEventListener('input', paint);
    paint();
    row.appendChild(head);
    row.appendChild(input);
    return row;
  }

  function makeChecks(c) {
    var row = el('div', 'aoko-ctrl aoko-checks');
    c.items.forEach(function (it) { row.appendChild(checkbox(it.label, it.checked)); });
    return row;
  }
  function makeCheck(c) {
    var row = el('div', 'aoko-ctrl aoko-checks');
    row.appendChild(checkbox(c.label, c.checked));
    return row;
  }
  function checkbox(label, checked) {
    var lab = el('label', 'aoko-check');
    var input = el('input', null, { type: 'checkbox' });
    if (checked) input.checked = true;
    var box = el('span', 'aoko-check-box', { 'aria-hidden': 'true' });
    var txt = el('span', 'aoko-check-txt', { text: label });
    lab.appendChild(input);
    lab.appendChild(box);
    lab.appendChild(txt);
    return lab;
  }

  function makeSwitchRow(c, mainState) {
    var row = el('div', 'aoko-ctrl aoko-switchrow');
    var txt = el('span', 'aoko-switchrow-label', { text: c.label });
    var tg = makeToggle(c.on, c.label);
    if (c.main) {
      mainState.has = true;
      mainState.on = !!c.on;
      tg.addEventListener('aoko-change', function (e) {
        mainState.on = e.detail;
        // trigger card-level gate/status refresh
        row.dispatchEvent(new CustomEvent('aoko-main', { detail: e.detail, bubbles: true }));
      });
    }
    row.appendChild(txt);
    row.appendChild(tg);
    return row;
  }

  function makeSelect(c) {
    var row = el('div', 'aoko-ctrl aoko-selectrow');
    if (c.label) row.appendChild(el('span', 'aoko-select-label', { text: c.label }));
    var sel = el('select', 'aoko-select', { 'aria-label': c.label || 'select' });
    c.options.forEach(function (opt, i) {
      var o = el('option', null, { value: String(i) });
      o.textContent = opt;
      if (i === (c.index || 0)) o.selected = true;
      sel.appendChild(o);
    });
    row.appendChild(sel);
    return row;
  }

  function makeButton(label) {
    var row = el('div', 'aoko-ctrl');
    var b = el('button', 'aoko-btn', { type: 'button' });
    b.textContent = label;
    row.appendChild(b);
    return row;
  }
  function makeButtons(items) {
    var row = el('div', 'aoko-ctrl aoko-btnrow');
    items.forEach(function (label) {
      var b = el('button', 'aoko-btn', { type: 'button' });
      b.textContent = label;
      row.appendChild(b);
    });
    return row;
  }

  function makeNote(text, warn) {
    return el('p', 'aoko-note' + (warn ? ' warn' : ''), { text: text });
  }

  function makeKeybinds(items) {
    var row = el('div', 'aoko-ctrl aoko-keybinds');
    items.forEach(function (name) {
      var b = el('button', 'aoko-keybind', { type: 'button' });
      b.innerHTML = '<span>' + name + '</span><em>None</em>';
      b.addEventListener('click', function () {
        var em = b.querySelector('em');
        if (b.classList.contains('listening')) return;
        b.classList.add('listening');
        em.textContent = '...';
        var onKey = function (e) {
          e.preventDefault();
          b.classList.remove('listening');
          em.textContent = e.key === 'Escape' ? 'None' : keyName(e);
          document.removeEventListener('keydown', onKey, true);
        };
        document.addEventListener('keydown', onKey, true);
      });
      row.appendChild(b);
    });
    return row;
  }
  function keyName(e) {
    if (e.code && e.code.indexOf('Key') === 0) return e.code.slice(3);
    if (e.code && e.code.indexOf('Digit') === 0) return e.code.slice(5);
    if (e.key === ' ') return 'Space';
    return e.key.length === 1 ? e.key.toUpperCase() : e.key;
  }

  function makePalettes() {
    var row = el('div', 'aoko-ctrl aoko-palettes');
    Object.keys(DATA.palettes).forEach(function (name) {
      var p = DATA.palettes[name];
      var card = el('button', 'aoko-swatchcard', { type: 'button', 'data-palette': name });
      var title = el('span', 'aoko-swatchcard-name', { text: name });
      var swatches = el('span', 'aoko-swatches');
      [p.bg, p.panel, p.line, p.accent].forEach(function (hex) {
        var s = el('span', 'aoko-swatch');
        s.style.background = hex;
        swatches.appendChild(s);
      });
      card.appendChild(title);
      card.appendChild(swatches);
      card.addEventListener('click', function () { applyPalette(name); });
      row.appendChild(card);
    });
    var cur = el('p', 'aoko-current', { 'data-role': 'palette-label' });
    cur.innerHTML = 'Current palette: <b>' + currentPalette + '</b>';
    row.appendChild(cur);
    return row;
  }

  function makeModStyles() {
    var row = el('div', 'aoko-ctrl aoko-modstyles');
    DATA.moduleStyles.forEach(function (name) {
      var b = el('button', 'aoko-modstyle', { type: 'button', 'data-style': name });
      b.innerHTML = '<span class="aoko-modstyle-name">' + name +
        '</span><span class="aoko-modstyle-demo ms-' + name.toLowerCase() +
        '">Autoclicker</span>';
      if (name === currentModStyle) b.classList.add('active');
      b.addEventListener('click', function () {
        currentModStyle = name;
        row.querySelectorAll('.aoko-modstyle').forEach(function (x) {
          x.classList.toggle('active', x === b);
        });
        var lbl = row.querySelector('[data-role="modstyle-label"]');
        if (lbl) lbl.innerHTML = 'Current module list style: <b>' + name + '</b>';
      });
      row.appendChild(b);
    });
    var cur = el('p', 'aoko-current', { 'data-role': 'modstyle-label' });
    cur.innerHTML = 'Current module list style: <b>' + currentModStyle + '</b>';
    row.appendChild(cur);
    return row;
  }

  /* =====================================================================
   * THEMING
   * ===================================================================== */
  function applyPalette(name) {
    var p = DATA.palettes[name];
    if (!p || !win) return;
    currentPalette = name;
    win.style.setProperty('--w-bg', p.bg);
    win.style.setProperty('--w-panel', p.panel);
    win.style.setProperty('--w-line', p.line);
    win.style.setProperty('--w-accent', p.accent);
    win.style.setProperty('--w-text', p.text);
    win.style.setProperty('--w-muted', rgba(p.text, 0.60));
    win.style.setProperty('--w-dim', rgba(p.text, 0.40));
    win.style.setProperty('--w-accent-soft', rgba(p.accent, 0.16));
    win.style.setProperty('--w-hover', rgba(p.text, 0.05));
    // active swatch + label
    desktop.querySelectorAll('.aoko-swatchcard').forEach(function (s) {
      s.classList.toggle('active', s.getAttribute('data-palette') === name);
    });
    desktop.querySelectorAll('[data-role="palette-label"]').forEach(function (l) {
      l.innerHTML = 'Current palette: <b>' + name + '</b>';
    });
  }

  /* =====================================================================
   * WINDOW BEHAVIOR — drag, minimize, maximize, restore
   * ===================================================================== */
  function enableDrag(handle) {
    var dragging = false, sx = 0, sy = 0, ox = 0, oy = 0;
    handle.addEventListener('pointerdown', function (e) {
      if (e.target.closest('.aoko-winbtn')) return;
      if (win.classList.contains('maximized') || mobileQuery.matches) return;
      dragging = true;
      var r = win.getBoundingClientRect();
      // switch to absolute positioning based on current rect
      win.style.left = r.left + 'px';
      win.style.top = r.top + 'px';
      win.style.transform = 'none';
      ox = r.left; oy = r.top; sx = e.clientX; sy = e.clientY;
      win.classList.add('dragging');
      handle.setPointerCapture(e.pointerId);
    });
    handle.addEventListener('pointermove', function (e) {
      if (!dragging) return;
      var nx = ox + (e.clientX - sx);
      var ny = oy + (e.clientY - sy);
      var maxX = window.innerWidth - 80;
      var maxY = window.innerHeight - 60;
      nx = Math.max(-win.offsetWidth + 120, Math.min(nx, maxX));
      ny = Math.max(0, Math.min(ny, maxY));
      win.style.left = nx + 'px';
      win.style.top = ny + 'px';
    });
    function end(e) {
      if (!dragging) return;
      dragging = false;
      win.classList.remove('dragging');
      try { handle.releasePointerCapture(e.pointerId); } catch (_) {}
    }
    handle.addEventListener('pointerup', end);
    handle.addEventListener('pointercancel', end);
  }

  function minimize() {
    win.classList.add('minimized');
    if (taskApp) taskApp.classList.remove('active');
  }
  function restore() {
    win.classList.remove('minimized');
    if (taskApp) taskApp.classList.add('active');
    win.focus();
  }
  function toggleMaximize() {
    if (mobileQuery.matches) return;
    var maxed = win.classList.toggle('maximized');
    if (maxed) {
      winRect = { left: win.style.left, top: win.style.top, transform: win.style.transform };
      win.style.left = ''; win.style.top = ''; win.style.transform = '';
    } else if (winRect) {
      win.style.left = winRect.left; win.style.top = winRect.top; win.style.transform = winRect.transform;
    }
  }

  /* =====================================================================
   * OPEN / CLOSE + focus trap
   * ===================================================================== */
  function open() {
    buildDesktop();
    lastFocused = document.activeElement;
    savedScrollY = window.scrollY;
    document.body.classList.add('aoko-noscroll');
    desktop.hidden = false;
    // force reflow then animate in
    void desktop.offsetWidth;
    desktop.classList.add('open');
    // reset window state on open
    win.classList.remove('minimized');
    if (taskApp) taskApp.classList.add('active');
    if (mobileQuery.matches) win.classList.add('maximized');
    selectTab(activeTab);
    setTimeout(function () { win.focus(); }, reduceMotion ? 0 : 60);
    document.addEventListener('keydown', onKeydown, true);
  }

  function close() {
    desktop.classList.remove('open');
    document.removeEventListener('keydown', onKeydown, true);
    document.body.classList.remove('aoko-noscroll');
    var finish = function () {
      desktop.hidden = true;
      desktop.removeEventListener('transitionend', finish);
      if (lastFocused && lastFocused.focus) lastFocused.focus();
    };
    if (reduceMotion) finish();
    else {
      var done = false;
      desktop.addEventListener('transitionend', function h() {
        if (done) return; done = true; desktop.removeEventListener('transitionend', h); finish();
      });
      setTimeout(function () { if (!done) { done = true; finish(); } }, 400);
    }
  }

  function onKeydown(e) {
    if (e.key === 'Escape') { e.preventDefault(); close(); return; }
    if (e.key === 'Tab') trapFocus(e);
  }
  function trapFocus(e) {
    var f = desktop.querySelectorAll(
      'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])'
    );
    var list = Array.prototype.filter.call(f, function (n) {
      return n.offsetParent !== null; // visible
    });
    if (!list.length) return;
    var first = list[0], last = list[list.length - 1];
    if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
    else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
  }

  launchBtn.addEventListener('click', open);
})();
