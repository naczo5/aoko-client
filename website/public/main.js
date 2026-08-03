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

/* ---- moonlight + forest depth ---- */
(function () {
  const scene = document.getElementById('site-scene');
  if (!scene) return;

  const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  const clamp = (value, min, max) => Math.min(Math.max(value, min), max);

  function paint(progress) {
    scene.style.setProperty('--scene-progress', progress.toFixed(4));
    scene.style.setProperty('--moon-shift-x', `${(-progress * 5.2).toFixed(2)}vw`);
    scene.style.setProperty('--moon-shift-y', `${(progress * 7.5).toFixed(2)}vh`);
    scene.style.setProperty('--light-strength', (1 - progress * 0.48).toFixed(4));
    scene.style.setProperty('--light-sweep', `${(progress * 18).toFixed(2)}deg`);
    scene.style.setProperty('--moon-brightness', (1.08 - progress * 0.18).toFixed(4));
    scene.style.setProperty('--moon-glow', (0.34 - progress * 0.22).toFixed(4));
    scene.style.setProperty('--moon-halo', (0.19 - progress * 0.12).toFixed(4));
    scene.style.setProperty('--scene-opacity', (1 - progress * 0.08).toFixed(4));
    scene.style.setProperty('--haze-opacity', (0.74 - progress * 0.14).toFixed(4));
    scene.style.setProperty('--beam-one-opacity', (0.46 - progress * 0.32).toFixed(4));
    scene.style.setProperty('--beam-two-opacity', (0.32 - progress * 0.24).toFixed(4));
    scene.style.setProperty('--stars-one-y', `${(-progress * 1.5).toFixed(2)}vh`);
    scene.style.setProperty('--stars-two-y', `${(-progress * 3).toFixed(2)}vh`);
    scene.style.setProperty('--haze-y', `${(-progress * 2).toFixed(2)}vh`);
    scene.style.setProperty('--forest-far-y', `${(-progress * 4).toFixed(2)}vh`);
    scene.style.setProperty('--forest-mid-y', `${(-progress * 8).toFixed(2)}vh`);
    scene.style.setProperty('--forest-near-y', `${(-progress * 15).toFixed(2)}vh`);
  }

  if (reduceMotion) {
    paint(0);
    return;
  }

  let progress = 0;
  let target = 0;
  let frame = 0;

  function readScrollProgress() {
    const sceneDistance = Math.max(window.innerHeight * 1.1, 760);
    return clamp(window.scrollY / sceneDistance, 0, 1);
  }

  function render() {
    progress += (target - progress) * 0.13;
    if (Math.abs(target - progress) < 0.001) progress = target;
    paint(progress);

    if (progress !== target) {
      frame = window.requestAnimationFrame(render);
    } else {
      frame = 0;
    }
  }

  function queueRender() {
    target = readScrollProgress();
    if (!frame) frame = window.requestAnimationFrame(render);
  }

  target = readScrollProgress();
  progress = target;
  paint(progress);
  window.addEventListener('scroll', queueRender, { passive: true });
  window.addEventListener('resize', queueRender, { passive: true });
})();

/* ---- scoop copy buttons ---- */
(function () {
  function setupCopyButton(btnId, containerId) {
    const copyBtn = document.getElementById(btnId);
    const cmdContainer = document.getElementById(containerId);
    if (!copyBtn || !cmdContainer) return;

    copyBtn.addEventListener('click', function () {
      const cmdEls = cmdContainer.querySelectorAll('.t-cmd');
      let textToCopy = '';
      if (cmdEls.length > 0) {
        textToCopy = Array.from(cmdEls).map(function (el) { return el.textContent.trim(); }).join('\n');
      } else {
        textToCopy = cmdContainer.innerText || cmdContainer.textContent;
      }
      navigator.clipboard.writeText(textToCopy.trim()).then(function () {
        copyBtn.classList.add('copied');
        copyBtn.textContent = 'copied!';
        setTimeout(function () {
          copyBtn.classList.remove('copied');
          copyBtn.textContent = 'copy';
        }, 2000);
      }).catch(function (err) {
        console.error('Failed to copy: ', err);
      });
    });
  }

  setupCopyButton('copy-scoop-cmd', 'scoop-cmd-text');
  setupCopyButton('copy-scoop-install-cmd', 'scoop-install-cmd-text');
})();

/* ---- scoop help modal ---- */
(function () {
  const modal = document.getElementById('scoop-modal');
  const toggleBtn = document.getElementById('scoop-help-toggle');
  const closeBtn = document.getElementById('scoop-modal-close-btn');
  const closeBg = document.getElementById('scoop-modal-close-bg');
  if (!modal || !toggleBtn) return;

  function openModal() {
    modal.removeAttribute('hidden');
    document.body.style.overflow = 'hidden';
  }

  function closeModal() {
    modal.setAttribute('hidden', '');
    document.body.style.overflow = '';
  }

  toggleBtn.addEventListener('click', openModal);
  if (closeBtn) closeBtn.addEventListener('click', closeModal);
  if (closeBg) closeBg.addEventListener('click', closeModal);

  document.addEventListener('keydown', function (e) {
    if (e.key === 'Escape' && !modal.hasAttribute('hidden')) {
      closeModal();
    }
  });
})();

