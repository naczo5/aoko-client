/* ---- scroll reveal ---- */
(function () {
  if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;
  const items = document.querySelectorAll(
    '.section-head, .reg-group, .node, .wire, .col, .dl-block'
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
