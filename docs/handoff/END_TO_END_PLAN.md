# 26-step end-to-end acceptance plan (not executed)

All steps must run against disposable repositories with independent identities and credentials, verified isolation, authenticated policy, safe storage, and explicit evidence for every result. The present implementation cannot execute this scenario.

1. Create disposable Git repository and initial commit.
2. Initialize a protected RIT registry after security gates pass.
3. Bootstrap the first administrator securely.
4. Create two human principals and an agent principal.
5. Issue short-lived credentials via secure files.
6. Assign least-privilege repository-scoped roles.
7. Verify deny/default-deny and invalid credentials.
8. Register the Git repository without mutating history.
9. Create a task with authorized participants.
10. Create isolated workspaces for both participants.
11. Validate OS sandbox and resource limits for agent execution.
12. Make independent changes in both workspaces.
13. Reconcile observed Git changes, staged changes and commits.
14. Create and verify complete checkpoints.
15. Verify partial restore preview and newer-work conflict detection.
16. Safely restore a selected file in disposable workspace.
17. Create two submissions bound to source/base/target.
18. Queue first submission and prepare candidate.
19. Run candidate validation and bind results to commit.
20. Obtain independent, authorized candidate-bound review.
21. Publish using target compare-and-swap; preserve source branch.
22. Observe target movement and rebase/reprepare second candidate.
23. Detect conflicts and request human semantic review without overwriting.
24. Analyze evidenced partial impact and recommended tests.
25. Exercise authenticated CLI/MCP and Rust/TypeScript/Python SDK interoperability, including denial paths.
26. Inject process death and concurrent changes; verify audit, backup, recovery, cleanup and Git-baseline results across native supported platforms.
