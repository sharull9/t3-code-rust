# Usage limits and bundled Geist

The full Settings page is already implemented. This change adds the subscription quota view from upstream, makes Usage easier to find, switches native typography to Geist, and replaces generic provider glyphs with SVG marks.

Usage opens from the labeled button in the sidebar footer. Its segmented controls offer Cost, Tokens and Limits. Cost and Tokens retain the 7/30/90-day controls and report cache. Limits has its own refresh state and a five-minute refresh interval while visible; it preserves the history range when switching tabs.

Quota data comes from `ServerProvider.usageLimits` and published `usageLimitSources` snapshots, with partial config-stream events merged into the current snapshot. Refresh calls `server.refreshProviders` with `refreshModels: false`. Matching emails or credential fingerprints deduplicate native and source accounts; the freshest quota wins while native names and the last reported credit balance remain. Pools group by driver, window kind and stable window ID. Their percentages average only accounts reporting that window, matching upstream. Missing clocks cannot affect pace. Account addresses stay redacted in the default view.

Cards show remaining quota, account bars, reset countdowns, pace and reported reset-credit balances. They wrap at narrow widths. Failures display beside the last reported data; unsupported subscriptions show an empty state or a provider-owned usage link. Server changes clear quota state, and request IDs reject old completions. Reset-credit redemption, quota-source editing and usage aggregation across multiple connected environments remain in the migration plan.

Geist and Geist Mono v1.7.2 are embedded with the upstream OFL license. Registration precedes GPUI component initialization. Both appearances use Geist for interface text and Geist Mono for code, diffs and terminal output. SVG marks are shared by Settings, Usage and existing provider-instance marks, preserving distinguishing badges. Asset sources are recorded in `crates/t3-gpui/assets/README.md`.

Validation covers account deduplication, snapshot/event projection, window pooling, missing clocks, failures, tab interactions, pending/offline refresh guards, retained history selection and narrow quota-card bounds. All 41 client tests, 59 native unit/UI tests and the client doctest passed. The Windows executable built successfully. UI tests verify state, interactions and layout bounds; pixel equivalence has not been checked. Native builds embed the assets without runtime downloads.
