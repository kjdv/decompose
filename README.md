# decompose
Service orchestration not depending on docker, optimised for dev.


## Milestones

- [x] Start one program provided by toml config
- [x] Start multiple programs provided by toml config
- [x] Redirect output
- [ ] separate start from ready, by open port, stdout regex
- [ ] respect cwd
- [ ] Selectively start
- [ ] Provisioning: separate build and run steps
- [ ] Timeout on hanging starts
- [x] Ordered destruction -> already done, programs stop in reverse order
