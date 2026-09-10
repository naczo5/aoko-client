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
        const textSpan = copyBtn.querySelector('.copy-text');
        if (textSpan) textSpan.textContent = 'copied!';
        else copyBtn.textContent = 'copied!';
        setTimeout(function () {
          copyBtn.classList.remove('copied');
          if (textSpan) textSpan.textContent = 'copy';
          else copyBtn.textContent = 'copy';
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

/* ---- video preview modal & custom player ---- */
(function () {
  const trigger = document.getElementById('preview-video-trigger');
  const modal = document.getElementById('video-preview-modal');
  const backdrop = document.getElementById('video-modal-backdrop');
  const closeBtn = document.getElementById('video-modal-close');
  const container = document.getElementById('video-player-container');
  const video = document.getElementById('modal-showcase-video');
  const centerBtn = document.getElementById('video-center-play-btn');
  const controlsBar = document.getElementById('video-custom-controls');
  const playBtn = document.getElementById('video-ctrl-play');
  const timeline = document.getElementById('video-timeline');
  const trackProgress = document.getElementById('video-track-progress');
  const trackBuffered = document.getElementById('video-track-buffered');
  const trackThumb = document.getElementById('video-track-thumb');
  const timeTooltip = document.getElementById('video-time-tooltip');
  const timeCurrent = document.getElementById('video-time-current');
  const timeDuration = document.getElementById('video-time-duration');
  const muteBtn = document.getElementById('video-ctrl-mute');
  const volumeSlider = document.getElementById('video-volume-slider');
  const loopBtn = document.getElementById('video-ctrl-loop');
  const fsBtn = document.getElementById('video-ctrl-fullscreen');

  if (!trigger || !modal || !video || !container) return;

  let isDraggingTimeline = false;
  let idleTimer = null;

  function formatTime(sec) {
    if (isNaN(sec) || sec < 0) return '0:00';
    const m = Math.floor(sec / 60);
    const s = Math.floor(sec % 60);
    return `${m}:${s < 10 ? '0' : ''}${s}`;
  }

  function triggerCenterPulse(isPlay) {
    if (!centerBtn) return;
    centerBtn.classList.remove('pulse', 'show-play', 'show-pause');
    void centerBtn.offsetWidth; // force reflow
    centerBtn.classList.add(isPlay ? 'show-play' : 'show-pause', 'pulse');
  }

  function togglePlay(fromScreenClick) {
    if (video.paused || video.ended) {
      const playPromise = video.play();
      if (playPromise !== undefined) {
        playPromise.catch(function () {});
      }
      if (fromScreenClick) triggerCenterPulse(true);
    } else {
      video.pause();
      if (fromScreenClick) triggerCenterPulse(false);
    }
  }

  function updateProgress() {
    if (!video.duration || isDraggingTimeline) return;
    const pct = (video.currentTime / video.duration) * 100;
    if (trackProgress) trackProgress.style.width = `${pct}%`;
    if (trackThumb) trackThumb.style.left = `${pct}%`;
    if (timeCurrent) timeCurrent.textContent = formatTime(video.currentTime);
    if (timeline) timeline.setAttribute('aria-valuenow', Math.round(pct));
  }

  function updateBuffered() {
    if (!video.duration || !trackBuffered) return;
    if (video.buffered.length > 0) {
      const bufferedEnd = video.buffered.end(video.buffered.length - 1);
      const pct = (bufferedEnd / video.duration) * 100;
      trackBuffered.style.width = `${pct}%`;
    }
  }

  function seekFromEvent(e) {
    if (!timeline || !video.duration) return;
    const rect = timeline.getBoundingClientRect();
    const pos = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
    video.currentTime = pos * video.duration;
    const pct = pos * 100;
    if (trackProgress) trackProgress.style.width = `${pct}%`;
    if (trackThumb) trackThumb.style.left = `${pct}%`;
    if (timeCurrent) timeCurrent.textContent = formatTime(video.currentTime);
  }

  function updateTooltip(e) {
    if (!timeline || !timeTooltip || !video.duration) return;
    const rect = timeline.getBoundingClientRect();
    const pos = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
    timeTooltip.textContent = formatTime(pos * video.duration);
    timeTooltip.style.left = `${pos * 100}%`;
  }

  function syncVolumeState() {
    const isMuted = video.muted || video.volume === 0;
    container.classList.toggle('is-muted', isMuted);
    if (volumeSlider) {
      volumeSlider.value = isMuted ? 0 : video.volume;
    }
  }

  function toggleFullscreen() {
    if (document.fullscreenElement) {
      document.exitFullscreen().catch(function () {});
    } else {
      (container.requestFullscreen || container.webkitRequestFullscreen || container.mozRequestFullScreen || container.msRequestFullscreen).call(container).catch(function () {});
    }
  }

  function resetIdleTimer() {
    container.classList.remove('is-idle');
    clearTimeout(idleTimer);
    if (!video.paused) {
      idleTimer = setTimeout(function () {
        if (!container.matches(':hover') || !controlsBar?.matches(':hover')) {
          container.classList.add('is-idle');
        }
      }, 2400);
    }
  }

  // Video playback events
  video.addEventListener('play', function () {
    container.classList.add('is-playing');
    container.classList.remove('is-paused');
    resetIdleTimer();
  });

  video.addEventListener('pause', function () {
    container.classList.remove('is-playing');
    container.classList.add('is-paused');
    container.classList.remove('is-idle');
    clearTimeout(idleTimer);
  });

  video.addEventListener('timeupdate', updateProgress);
  video.addEventListener('progress', updateBuffered);

  video.addEventListener('loadedmetadata', function () {
    if (timeDuration) timeDuration.textContent = formatTime(video.duration);
    updateProgress();
    updateBuffered();
  });

  video.addEventListener('click', function () {
    togglePlay(true);
  });

  if (centerBtn) {
    centerBtn.addEventListener('click', function (e) {
      e.stopPropagation();
      togglePlay(true);
    });
  }

  if (playBtn) {
    playBtn.addEventListener('click', function (e) {
      e.stopPropagation();
      togglePlay(false);
    });
  }

  // Scrubber / Timeline events
  if (timeline) {
    timeline.addEventListener('mousemove', updateTooltip);

    timeline.addEventListener('pointerdown', function (e) {
      isDraggingTimeline = true;
      timeline.classList.add('is-dragging');
      timeline.setPointerCapture(e.pointerId);
      seekFromEvent(e);
      resetIdleTimer();
    });

    timeline.addEventListener('pointermove', function (e) {
      if (!isDraggingTimeline) return;
      seekFromEvent(e);
      updateTooltip(e);
    });

    function endDrag(e) {
      if (!isDraggingTimeline) return;
      isDraggingTimeline = false;
      timeline.classList.remove('is-dragging');
      try {
        timeline.releasePointerCapture(e.pointerId);
      } catch (_) {}
    }

    timeline.addEventListener('pointerup', endDrag);
    timeline.addEventListener('pointercancel', endDrag);

    timeline.addEventListener('keydown', function (e) {
      if (e.key === 'ArrowLeft') {
        e.preventDefault();
        video.currentTime = Math.max(0, video.currentTime - 2);
      } else if (e.key === 'ArrowRight') {
        e.preventDefault();
        video.currentTime = Math.min(video.duration || 0, video.currentTime + 2);
      }
    });
  }

  // Volume & Mute events
  if (muteBtn) {
    muteBtn.addEventListener('click', function (e) {
      e.stopPropagation();
      video.muted = !video.muted;
      if (!video.muted && video.volume === 0) {
        video.volume = 1;
      }
      syncVolumeState();
      resetIdleTimer();
    });
  }

  if (volumeSlider) {
    volumeSlider.addEventListener('input', function (e) {
      const val = parseFloat(e.target.value);
      video.volume = val;
      video.muted = val === 0;
      syncVolumeState();
      resetIdleTimer();
    });
  }

  // Loop toggle
  if (loopBtn) {
    video.loop = true;
    loopBtn.addEventListener('click', function (e) {
      e.stopPropagation();
      video.loop = !video.loop;
      loopBtn.classList.toggle('is-active', video.loop);
      loopBtn.setAttribute('aria-label', video.loop ? 'Loop enabled' : 'Loop disabled');
      resetIdleTimer();
    });
  }

  // Fullscreen events
  if (fsBtn) {
    fsBtn.addEventListener('click', function (e) {
      e.stopPropagation();
      toggleFullscreen();
    });
  }

  document.addEventListener('fullscreenchange', function () {
    const isFs = !!document.fullscreenElement;
    container.classList.toggle('is-fullscreen', isFs);
  });

  // Idle controls hiding
  container.addEventListener('mousemove', resetIdleTimer);
  container.addEventListener('pointerdown', resetIdleTimer);
  container.addEventListener('mouseleave', function () {
    if (!video.paused) {
      container.classList.add('is-idle');
    }
  });

  if (controlsBar) {
    controlsBar.addEventListener('mouseenter', function () {
      clearTimeout(idleTimer);
      container.classList.remove('is-idle');
    });
    controlsBar.addEventListener('mouseleave', resetIdleTimer);
  }

  // Modal open / close handlers
  function openVideoModal() {
    modal.removeAttribute('hidden');
    void modal.offsetWidth;
    modal.classList.add('open');
    document.body.classList.add('aoko-noscroll');
    container.classList.remove('is-idle');
    container.classList.add('is-paused');

    video.currentTime = 0;
    syncVolumeState();
    if (video.duration) {
      timeDuration.textContent = formatTime(video.duration);
    }
    updateProgress();

    const playPromise = video.play();
    if (playPromise !== undefined) {
      playPromise.catch(function () {
        // Autoplay may be blocked by browser policy
      });
    }
  }

  function closeVideoModal() {
    if (document.fullscreenElement) {
      document.exitFullscreen().catch(function () {});
    }
    video.pause();
    modal.classList.remove('open');
    document.body.classList.remove('aoko-noscroll');
    container.classList.remove('is-idle');
    clearTimeout(idleTimer);

    setTimeout(function () {
      if (!modal.classList.contains('open')) {
        modal.setAttribute('hidden', '');
      }
    }, 250);
  }

  window.openVideoModal = openVideoModal;
  window.closeVideoModal = closeVideoModal;

  trigger.addEventListener('click', openVideoModal);
  trigger.addEventListener('keydown', function (e) {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      openVideoModal();
    }
  });

  if (closeBtn) closeBtn.addEventListener('click', closeVideoModal);
  if (backdrop) backdrop.addEventListener('click', closeVideoModal);

  document.addEventListener('keydown', function (e) {
    if (modal.hasAttribute('hidden') || !modal.classList.contains('open')) return;

    if (e.key === 'Escape') {
      if (document.fullscreenElement) {
        document.exitFullscreen().catch(function () {});
        return;
      }
      e.preventDefault();
      closeVideoModal();
      return;
    }

    // Keyboard shortcuts inside active video modal
    if (e.target.tagName === 'INPUT' && e.target.type !== 'range') return;

    if (e.key === ' ' || e.key.toLowerCase() === 'k') {
      e.preventDefault();
      togglePlay(true);
      resetIdleTimer();
    } else if (e.key.toLowerCase() === 'f') {
      e.preventDefault();
      toggleFullscreen();
    } else if (e.key.toLowerCase() === 'm') {
      e.preventDefault();
      video.muted = !video.muted;
      if (!video.muted && video.volume === 0) video.volume = 1;
      syncVolumeState();
      resetIdleTimer();
    } else if (e.key === 'ArrowLeft') {
      e.preventDefault();
      video.currentTime = Math.max(0, video.currentTime - 2);
      resetIdleTimer();
    } else if (e.key === 'ArrowRight') {
      e.preventDefault();
      video.currentTime = Math.min(video.duration || 0, video.currentTime + 2);
      resetIdleTimer();
    }
  });
})();

