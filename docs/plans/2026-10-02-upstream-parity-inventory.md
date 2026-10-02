# Upstream contract parity inventory

Upstream revision `54084ae1e6c32809db040e4fa571c80fdf2d8ae4`, inspected 2026-10-02. Native baseline `f7c4677`, plus the full Settings page in this turn.

This checklist accompanies the [migration plan](2026-10-02-full-t3-code-migration.md). It is extracted from the public method registries and selected authoritative schemas, rather than inferred from menu labels.

Coverage: 153 public RPC method entries; 65 device preferences fields; 43 server settings fields; 18 project overrides fields; 6 storage policies fields; 33 environment capabilities fields.

The phase is the first implementation owner. Cross-environment completion also depends on P9. A Rust literal reference means the exact method name occurs in current source; it does not prove that its inputs, results, events or UI cover the full contract. No reference is a migration gap to investigate, not proof that the server lacks the capability.

Top-level schema fields are listed below. Their nested payloads, provider-specific settings, allowed values, defaults and null/omission semantics must be checked in the linked source. This list does not enumerate every internal event, desktop IPC call or mobile-only setting. Those are covered by feature tasks and platform dispositions in the main plan.

## Public RPC methods

| Method | Rust literal reference | Owner |
| --- | --- | --- |
| [`projects.list`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P6 |
| [`projects.add`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P6 |
| [`projects.remove`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P6 |
| [`projects.listEntries`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | Present; verify workflow | P6 |
| [`projects.readFile`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | Present; verify workflow | P6 |
| [`projects.searchContents`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P6 |
| [`projects.searchEntries`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P6 |
| [`projects.writeFile`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P6 |
| [`projects.ensureScratch`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P6 |
| [`projects.createNew`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P6 |
| [`shell.openInEditor`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P6 |
| [`filesystem.browse`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | Present; verify workflow | P6 |
| [`agentSessions.scan`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P4 |
| [`agentSessions.import`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P4 |
| [`assets.createUrl`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | Present; verify workflow | P5 |
| [`attachments.createUploadUrl`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | Present; verify workflow | P5 |
| [`attachments.delete`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P5 |
| [`provider.uploadFeedback`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P4 |
| [`provider.auth.start`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P4 |
| [`provider.consumeResetCredit`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P4 |
| [`provider.auth.complete`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P4 |
| [`provider.chatgpt.reconnect-profile`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P4 |
| [`provider.chatgpt.import-profile`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P4 |
| [`provider.chatgpt.handoff.subscribe`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P4 |
| [`provider.codex.auth-callback.subscribe`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P4 |
| [`provider.auth.respond`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P4 |
| [`provider.auth.cancel`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P4 |
| [`provider.auth.logout`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P4 |
| [`provider.auth.subscribe`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P4 |
| [`provider.install.start`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P4 |
| [`provider.install.cancel`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P4 |
| [`provider.install.subscribe`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P4 |
| [`provider.install.remove`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P4 |
| [`vcs.pull`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P6 |
| [`vcs.refreshStatus`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | Present; verify workflow | P6 |
| [`vcs.listRefs`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | Present; verify workflow | P6 |
| [`vcs.createWorktree`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P6 |
| [`vcs.removeWorktree`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P6 |
| [`vcs.createRef`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P6 |
| [`vcs.switchRef`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | Present; verify workflow | P6 |
| [`vcs.init`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P6 |
| [`git.runStackedAction`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P7 |
| [`git.resolvePullRequest`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P7 |
| [`git.preparePullRequestThread`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P7 |
| [`review.getDiffPreview`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | Present; verify workflow | P7 |
| [`review.getDiffFileContents`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P7 |
| [`terminal.open`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | Present; verify workflow | P6 |
| [`terminal.attach`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | Present; verify workflow | P6 |
| [`terminal.write`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | Present; verify workflow | P6 |
| [`terminal.resize`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P6 |
| [`terminal.clear`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P6 |
| [`terminal.restart`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | Present; verify workflow | P6 |
| [`terminal.close`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | Present; verify workflow | P6 |
| [`preview.open`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P8 |
| [`preview.navigate`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P8 |
| [`preview.resize`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P8 |
| [`preview.refresh`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P8 |
| [`preview.close`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P8 |
| [`preview.list`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P8 |
| [`preview.reportStatus`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P8 |
| [`previewAutomation.connect`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P8 |
| [`previewAutomation.respond`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P8 |
| [`previewAutomation.focusHost`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P8 |
| [`device.configure`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P8 |
| [`device.list`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P8 |
| [`device.testHost`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P8 |
| [`device.open`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P8 |
| [`device.close`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P8 |
| [`device.shutdown`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P8 |
| [`device.detail`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P8 |
| [`device.action`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P8 |
| [`server.probe`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P1 |
| [`server.getConfig`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | Present; verify workflow | P1 |
| [`server.refreshProviders`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P4 |
| [`server.updateProvider`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P4 |
| [`server.updateServer`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P10 |
| [`server.updateServerWithProgress`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P10 |
| [`server.commitDesktopUpdate`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P10 |
| [`server.upsertKeybinding`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P3 |
| [`server.removeKeybinding`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P3 |
| [`server.getSettings`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P2 |
| [`server.updateSettings`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P2 |
| [`server.discoverSourceControl`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P7 |
| [`server.getTraceDiagnostics`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P10 |
| [`server.getProcessDiagnostics`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P10 |
| [`server.getHostResources`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P10 |
| [`server.getProcessResourceHistory`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P10 |
| [`server.getResourceTelemetryHistory`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P10 |
| [`server.retryResourceTelemetry`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P10 |
| [`server.signalProcess`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P10 |
| [`server.reportClientActivity`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P10 |
| [`server.reportHostPowerState`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P10 |
| [`server.getBackgroundPolicy`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P10 |
| [`server.getUsageSummary`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | Present; verify workflow | P7 |
| [`server.refreshUsageRates`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | Present; verify workflow | P7 |
| [`cloud.getRelayClientStatus`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P9 |
| [`cloud.installRelayClient`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P9 |
| [`pullRequests.list`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P7 |
| [`pullRequests.listStats`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P7 |
| [`pullRequests.summary`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P7 |
| [`pullRequests.routing`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P7 |
| [`pullRequests.routingIdentity`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P7 |
| [`pullRequests.stack`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P7 |
| [`pullRequests.linkedThreads`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P7 |
| [`pullRequests.detail`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P7 |
| [`pullRequests.preview`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P7 |
| [`pullRequests.activity`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P7 |
| [`pullRequests.threadComments`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P7 |
| [`pullRequests.diffFileContents`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P7 |
| [`pullRequests.filesViewed`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P7 |
| [`pullRequests.setFilesViewed`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P7 |
| [`pullRequests.runAction`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P7 |
| [`pullRequests.update`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P7 |
| [`pullRequests.comment`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P7 |
| [`pullRequests.updateComment`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P7 |
| [`pullRequests.submitReview`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P7 |
| [`pullRequests.replyToThread`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P7 |
| [`pullRequests.setThreadResolution`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P7 |
| [`pullRequests.setReaction`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P7 |
| [`pullRequests.invalidate`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P7 |
| [`pullRequests.subscribeRefreshes`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P7 |
| [`pullRequests.reviewerCandidates`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P7 |
| [`pullRequests.requestReviewers`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P7 |
| [`pullRequests.labelCandidates`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P7 |
| [`pullRequests.setLabels`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P7 |
| [`sourceControl.lookupRepository`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P6 |
| [`sourceControl.cloneRepository`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P6 |
| [`sourceControl.publishRepository`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P6 |
| [`projectClone.start`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P6 |
| [`projectClone.cancel`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P6 |
| [`projectClone.retry`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P6 |
| [`subscribeProjectClones`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P6 |
| [`subscribeVcsStatus`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P6 |
| [`subscribeWorktreeSetup`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P6 |
| [`worktreeSetup.cancel`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P6 |
| [`subscribeTerminalEvents`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P6 |
| [`subscribeTerminalMetadata`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P6 |
| [`subscribePreviewEvents`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P8 |
| [`subscribeDiscoveredLocalServers`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P9 |
| [`subscribeDeviceState`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P8 |
| [`subscribeServerConfig`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | Present; verify workflow | P1 |
| [`subscribeServerLifecycle`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P10 |
| [`subscribeAuthAccess`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P9 |
| [`subscribeBackgroundPolicy`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P10 |
| [`subscribeResourceTelemetry`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/rpc.ts) | No reference found | P10 |
| [`orchestration.dispatchCommand`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/orchestration.ts) | Present; verify workflow | P1 |
| [`orchestration.getWorkflowScript`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/orchestration.ts) | No reference found | P6 |
| [`orchestration.getTurnDiff`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/orchestration.ts) | No reference found | P7 |
| [`orchestration.getFullThreadDiff`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/orchestration.ts) | No reference found | P7 |
| [`orchestration.searchThreads`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/orchestration.ts) | No reference found | P3 |
| [`orchestration.getArchivedShellSnapshot`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/orchestration.ts) | Present; verify workflow | P1 |
| [`orchestration.subscribeShell`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/orchestration.ts) | Present; verify workflow | P1 |
| [`orchestration.subscribeThread`](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/orchestration.ts) | Present; verify workflow | P1 |

## Device preferences

[Authoritative schema](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/settings.ts).

| Field | Owner |
| --- | --- |
| `notificationMode` | P10 |
| `inAppNotificationsEnabled` | P10 |
| `diffColorScheme` | P2 |
| `chatWidth` | P2 |
| `loadBalancingEnabled` | P9 |
| `loadBalancingWeights` | P9 |
| `appearanceContrast` | P2 |
| `panelAnimationDurationMs` | P2 |
| `browserDefaultViewport` | P8 |
| `browserDefaultZoomFactor` | P8 |
| `browserDefaultAppearance` | P8 |
| `browserRecordingFrameRate` | P8 |
| `browserRecordingShowKeyPresses` | P8 |
| `browserRecordingShowMousePresses` | P8 |
| `browserLinkTarget` | P8 |
| `browserAutoShowFloatingPreview` | P8 |
| `browserProfiles` | P8 |
| `browserDefaultProfileId` | P8 |
| `confirmQuit` | P2 |
| `confirmThreadArchive` | P2 |
| `confirmThreadDelete` | P2 |
| `confirmThreadUnpin` | P2 |
| `dismissedProviderUpdateNotificationKeys` | P2 |
| `diffFilesCollapsed` | P2 |
| `diffIgnoreWhitespace` | P2 |
| `diffLayout` | P2 |
| `environmentIdentificationMode` | P2 |
| `glassOpacity` | P2 |
| `fontSizeInterface` | P2 |
| `fontSizePrompt` | P2 |
| `fontSizeCode` | P2 |
| `fontSizeTerminal` | P2 |
| `fontFamilyCode` | P2 |
| `fontFamilyComposer` | P2 |
| `fontFamilySans` | P2 |
| `fontFamilyTerminal` | P2 |
| `fontSmoothing` | P2 |
| `onboardingCompletedAt` | P2 |
| `favorites` | P4 |
| `providerModelPreferences` | P4 |
| `pullRequestMergeMethodOverrides` | P7 |
| `planModeEnabled` | P2 |
| `contextWindowMeterEnabled` | P2 |
| `composerCollapseOnScroll` | P2 |
| `composerRichTextEnabled` | P2 |
| `sendShortcut` | P2 |
| `followUpBehavior` | P2 |
| `proactivePanelsEnabled` | P2 |
| `showSkillsInSlashMenu` | P2 |
| `legacySidebarEnabled` | P2 |
| `sidebarWorkingShelfEnabled` | P2 |
| `sidebarProjectGroupingMode` | P2 |
| `sidebarProjectGroupingOverrides` | P2 |
| `sidebarProjectSortOrder` | P2 |
| `sidebarThreadSortOrder` | P2 |
| `sidebarThreadPreviewCount` | P2 |
| `timestampFormat` | P2 |
| `snapShotEnabled` | P10 |
| `snapShotIncludeAccessibility` | P10 |
| `snapShotShortcut` | P10 |
| `snapShotPlaySound` | P10 |
| `snapShotSound` | P10 |
| `snapShotFlash` | P10 |
| `snapShotAnimations` | P10 |
| `wordWrap` | P2 |

## Server settings

[Authoritative schema](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/settings.ts).

| Field | Owner |
| --- | --- |
| `worktreeCleanup` | P6 |
| `storageCleanup` | P6 |
| `responseStreamingMode` | P2 |
| `enableProviderUpdateChecks` | P2 |
| `continueThreadsAfterServerUpdate` | P2 |
| `enableAgentBrowserAccess` | P8 |
| `projectAgentBrowserAccessOverrides` | P8 |
| `defaultAutoPull` | P7 |
| `defaultProjectScripts` | P2 |
| `projectScriptOverrides` | P2 |
| `projectAutoPullOverrides` | P7 |
| `defaultModelSelection` | P2 |
| `defaultRuntimeMode` | P2 |
| `projectSettingsOverrides` | P2 |
| `projectSettingsFolded` | P2 |
| `enableAgentDeviceAccess` | P8 |
| `enableDeviceSupport` | P2 |
| `deviceOnboardingCompleted` | P8 |
| `deviceHosts` | P8 |
| `sidebarAutoSettleAfterDays` | P2 |
| `sidebarAutoSettleOnMerge` | P2 |
| `backgroundActivity` | P10 |
| `automaticGitFetchInterval` | P2 |
| `providerHealthRefreshInterval` | P4 |
| `backgroundActivityProfile` | P10 |
| `defaultTheme` | P2 |
| `defaultThemeSetAt` | P2 |
| `environmentIcon` | P9 |
| `defaultThreadEnvMode` | P2 |
| `newWorktreesStartFromOrigin` | P2 |
| `worktreeSubmodules` | P2 |
| `addProjectBaseDirectory` | P2 |
| `textGenerationModelSelection` | P2 |
| `sourceControlWritingStyle` | P7 |
| `sourceControlWriterModelSelection` | P7 |
| `pullRequestMergeMethod` | P7 |
| `providers` | P4 |
| `providerInstances` | P4 |
| `observability` | P10 |
| `bitbucket` | P7 |
| `usageLimitSources` | P4 |
| `cursorKeychainUsageEnabled` | P4 |
| `usagePriceOverrides` | P7 |

## Project overrides

[Authoritative schema](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/settings.ts).

| Field | Owner |
| --- | --- |
| `worktreeCleanup` | P6 |
| `defaultModelSelection` | P2 |
| `defaultRuntimeMode` | P2 |
| `defaultThreadEnvMode` | P2 |
| `newWorktreesStartFromOrigin` | P2 |
| `worktreeSubmodules` | P2 |
| `defaultAutoPull` | P7 |
| `defaultProjectScripts` | P2 |
| `enableAgentBrowserAccess` | P8 |
| `enableAgentDeviceAccess` | P8 |
| `textGenerationModelSelection` | P2 |
| `sourceControlWriterModelSelection` | P7 |
| `sourceControlWritingStyle` | P7 |
| `pullRequestMergeMethod` | P7 |
| `sidebarAutoSettleOnMerge` | P2 |
| `sidebarAutoSettleAfterDays` | P2 |
| `continueThreadsAfterServerUpdate` | P2 |
| `responseStreamingMode` | P2 |

## Storage policies

[Authoritative schema](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/settings.ts).

| Field | Owner |
| --- | --- |
| `worktreeAfterDays` | P6 |
| `worktreeOnMerge` | P6 |
| `worktreeOnDelete` | P6 |
| `worktreeUnchanged` | P6 |
| `browserArtifactsAfterDays` | P6 |
| `logsAfterDays` | P6 |

## Environment capabilities

[Authoritative schema](https://github.com/pingdotgg/t3code/blob/54084ae1e6c32809db040e4fa571c80fdf2d8ae4/packages/contracts/src/environment.ts).

| Field | Owner |
| --- | --- |
| `repositoryIdentity` | P1 |
| `connectionProbe` | P1 |
| `attachmentUploads` | P1 |
| `questionAttachments` | P1 |
| `fileAttachments` | P1 |
| `pullRequests` | P1 |
| `inlineMessageContext` | P1 |
| `requiredWorktreeBootstrap` | P1 |
| `threadSettlement` | P1 |
| `threadAutoSettlement` | P1 |
| `storageCleanup` | P1 |
| `projectWorktreeCleanup` | P1 |
| `threadRestartContinuation` | P1 |
| `projectSettingsOverrides` | P1 |
| `threadSnooze` | P1 |
| `environmentThemes` | P1 |
| `usageLimitSources` | P1 |
| `usagePriceOverrides` | P1 |
| `threadPinning` | P1 |
| `threadPinReorder` | P1 |
| `threadActiveReorder` | P1 |
| `threadAutoSettleOptOut` | P1 |
| `threadTitleRegeneration` | P1 |
| `threadPullRequestLinking` | P1 |
| `threadPullRequests` | P1 |
| `pullRequestStackActions` | P1 |
| `serverSelfUpdate` | P1 |
| `serverSelfUpdateProgress` | P1 |
| `serverUpdateThreadContinuation` | P1 |
| `agentActivityPublishing` | P1 |
| `projectCloneTracking` | P1 |
| `environmentIcon` | P1 |
| `desktopAppUpdate` | P1 |

## Update procedure

Refresh the cached upstream checkout with a fast-forward pull. Re-extract entries between the `WS_METHODS` and `ORCHESTRATION_WS_METHODS` declarations and their closing braces, and top-level two-space-indented fields in the five schema declarations named above. Compare added/removed fields and payload changes against this revision. Recheck the native method literals, then assign any new feature to a phase before implementing it.

Replace reference-only evidence with verified workflow evidence as migration work lands. For an intentionally unsupported platform feature, link its disposition instead of calling the feature complete.
