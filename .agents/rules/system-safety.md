---
description: Apply before changing host OS configuration, drivers, boot,
  device permissions or privileged services.
trigger: model_decision
---

# Host System Safety

Never install packages, change drivers, edit bootloader/kernel
parameters, alter systemd/udev/display-manager configuration, change
device permissions, or execute destructive commands without explicit
user approval.

Before proposing a change: 1. Explain why it is needed. 2. List exact
files, packages and commands affected. 3. Explain risk and rollback. 4.
Offer a non-invasive alternative where possible. 5. Wait for explicit
approval.
