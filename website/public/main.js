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

