/* aoko client — landing interactions */

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
