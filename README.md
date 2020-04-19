# decompose
Service orchestration not depending on docker, optimised for dev.


## Milestones

- [x] Start one program provided by toml config
- [x] Start multiple programs provided by toml config
- [x] Redirect output
- [x] separate start from ready, by open port, stdout regex
- [x] respect cwd
- [x] Selectively start
- [-] Provisioning: separate build and run steps - won't do (can be done by tasks)
- [x] Timeout on hanging starts
- [x] Ordered destruction -> already done, programs stop in reverse order
