# 40-step v2 end-to-end acceptance scenario — NOT RUN

All steps require disposable repositories, real providers, validated security and preserved evidence. No mocked result substitutes for production acceptance.

1. Initialize RIT.
2. Register the repository.
3. Inspect repository health.
4. Bootstrap owner identity.
5. Create five agent identities.
6. Assign different scoped permissions.
7. Create five workspaces.
8. Verify independent branches.
9. Assign tasks.
10. Assign task dependencies.
11. Connect or launch supported external agents.
12. Observe real agent activity.
13. Create checkpoints.
14. Modify overlapping files.
15. Detect overlaps.
16. Create submissions.
17. Build candidate integrations.
18. Detect textual conflicts.
19. Inspect structural conflicts.
20. Generate impact reports.
21. Recommend relevant tests.
22. Run validation.
23. Reject a submission whose tests fail.
24. Verify source work is preserved.
25. Correct the failing submission.
26. Resubmit.
27. Request human approval.
28. Publish safely.
29. Verify target branch.
30. Verify source branches.
31. Verify Git history.
32. Interrupt an operation.
33. Restart RIT.
34. Recover or reconcile the interrupted operation.
35. Verify audit records.
36. Verify API and MCP access.
37. Verify standard Git commands still work.
38. Verify repository integrity.
39. Verify workspace cleanup.
40. Verify no user work was lost.

Separate isolated tests for Git snapshots and five test-created worktrees do not constitute execution of these 40 steps.
