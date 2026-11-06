# SPEC-disks

## Objective

Read block devices, mount/unmount filesystems, and — behind an explicit
enablement flag — format and partition them. The destructive trio stays
off until an admin opts in.

## API surface

| Action | Backend |
|---|---|
| `list_disks` | `lsblk -J -o ...` |
| `mount_disk` | `udisksctl mount -b <device>` |
| `unmount_disk` | `udisksctl unmount -b <device>` |
| `format_disk` | `mkfs.<fstype>` (whitelisted types) |
| `partition_create` | `parted -s <dev> mkpart primary <start>MiB <end>MiB` |
| `partition_delete` | `parted -s <dev> rm <n>` |

- Capability: `udisks2` (lsblk + udisksctl); the destructive trio also
  requires `parted` and the matching `mkfs.*` tool at runtime
  (checked per action, reported as unavailable with a fix hint).
- Module switch: `disks`.
- Device must be an absolute `/dev/...` path, no `..`.
- Root commands.
- Destructive trio gated three ways: `disks_format = true` in
  `/etc/ursine/config.toml` (explicit enablement, default off,
  restart required — recorded in session-state key_decisions),
  `confirm_device` must equal `device` (exact device typed), and
  `ursinectl --yes`. Unsupported filesystem types are rejected from a
  fixed whitelist before any mkfs path is built.

## GUI

- Sidebar **Disks**.
- Device/partition list with size, filesystem, label, mountpoint.
- Mount/unmount buttons.
- When `format_enabled`: Format… on partitions (fstype dropdown +
  typed-device confirm), Partition… on disks (start/end MiB +
  typed-device confirm), Delete… on partitions (typed confirm); one
  flow at a time, cancelled on failure so typed input survives.
- Busy/error states.
