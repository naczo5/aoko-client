/* aoko client — landing interactions */

/* ---- injection console typewriter ---- */
(function () {
  const body = document.getElementById('console-body');
  if (!body) return;

  // Each line: prompt (pr), plain, or status. Rendered with light markup.
  const lines = [
    '<span class="pr">aoko@lunar</span>:<span class="ac">~</span>$ ./aoko --inject',
    '<span class="ok">[ ok ]</span> locating <span class="hi">javaw.exe</span> .............. pid 8842',
    '<span class="ok">[ ok ]</span> mapping jvm <span class="hi">(yarn → mojmap)</span> ..... 26.1 / 1.21 / 1.8.9',
    '<span class="ok">[ ok ]</span> injecting <span class="hi">bridge_261.dll</span> ....... attached',
    '<span class="ok">[ ok ]</span> handshake <span class="hi">tcp://127.0.0.1:25590</span> .. up',
    '<span class="ok">[ ok ]</span> loading modules ................ <span class="hi">17 online</span>',
    '<span class="ac">[ &gt;&gt; ]</span> overlay ready. gg.',
  ];

  const reduce = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

  if (reduce) {
    body.innerHTML = lines.join('\n');
    return;
  }

  // Type by walking the *rendered* text but preserving tags. We stream token by
  // token: build each line char-by-char from a tag-aware tokenizer.
  function tokenize(html) {
    // returns array of {tag:true,val} or {char}
    const out = [];
    let i = 0;
    while (i < html.length) {
      if (html[i] === '<') {
        const end = html.indexOf('>', i);
        out.push({ raw: html.slice(i, end + 1) });
        i = end + 1;
      } else if (html[i] === '&') {
        const end = html.indexOf(';', i);
        out.push({ raw: html.slice(i, end + 1), print: true });
        i = end + 1;
      } else {
        out.push({ raw: html[i], print: true });
        i++;
      }
    }
    return out;
  }

  const caret = '<span class="caret">_</span>';
  let done = '';
  let li = 0;

  function typeLine() {
    if (li >= lines.length) { body.innerHTML = done + caret; return; }
    const tokens = tokenize(lines[li]);
    let cur = '';
    let ti = 0;
    (function step() {
      // consume tags instantly, printable chars one at a time
      while (ti < tokens.length && !tokens[ti].print) {
        cur += tokens[ti].raw; ti++;
      }
      if (ti < tokens.length) {
        cur += tokens[ti].raw; ti++;
        body.innerHTML = done + cur + caret;
        setTimeout(step, 14);
      } else {
        done += cur + '\n';
        li++;
        setTimeout(typeLine, li === 1 ? 320 : 240);
      }
    })();
  }

  // start when console scrolls into view (or immediately if already visible)
  const io = new IntersectionObserver((entries, obs) => {
    entries.forEach((e) => {
      if (e.isIntersecting) { typeLine(); obs.disconnect(); }
    });
  }, { threshold: 0.4 });
  io.observe(body);
})();

/* ---- status date + session uptime ---- */
(function () {
  const dateEl = document.getElementById('status-date');
  if (dateEl) {
    dateEl.textContent = new Date().toLocaleDateString('en-US', {
      month: 'short', day: 'numeric', year: 'numeric',
    }).toLowerCase();
  }

  const up = document.getElementById('uptime');
  if (up) {
    const start = Date.now();
    const pad = (n) => String(n).padStart(2, '0');
    setInterval(() => {
      const s = Math.floor((Date.now() - start) / 1000);
      up.textContent = `${pad(Math.floor(s / 3600))}:${pad(Math.floor((s % 3600) / 60))}:${pad(s % 60)}`;
    }, 1000);
  }
})();

/* ---- scroll reveal ---- */
(function () {
  if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;
  const items = document.querySelectorAll(
    '.section-head, .reg-group, .stat, .node, .wire, .col, .dl-block, .hero-preview'
  );
  items.forEach((el) => el.classList.add('reveal'));
  const observer = new IntersectionObserver((entries) => {
    entries.forEach((entry) => {
      if (!entry.isIntersecting) return;
      entry.target.classList.add('visible');
      observer.unobserve(entry.target);
    });
  }, { threshold: 0.12 });
  items.forEach((el) => observer.observe(el));
})();
