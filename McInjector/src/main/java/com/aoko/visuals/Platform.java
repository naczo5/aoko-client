package com.aoko.visuals;

/** One game generation (legacy 1.8.9 or modern 26.x). Created and driven by {@link Visuals}. */
interface Platform {
    /**
     * Resolves every game class, field and method the platform needs. Returns null when
     * everything is available, or a status string: {@code unsupported:<why>} when the
     * game version cannot be driven at all, {@code partial:<what>} when some features
     * are missing.
     */
    String init(Library library);

    /** The settings changed. Must return quickly: queue game-thread work, do not block. */
    void onConfig(Config config, SkyState sky);

    /** Periodic repair and lazy loading, called from the bridge's worker thread. */
    void maintain(Config config, SkyState sky);

    /** Notes about the current selection (missing assets, failed loads), or "" when none. */
    String notes();

    void shutdown();
}
