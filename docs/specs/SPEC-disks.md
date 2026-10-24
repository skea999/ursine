# SPEC-disks

## Objective

Read block devices and mount/unmount filesystems through UDisks2. Formatting
and partitioning are out of scope for this version.

## API surface

| Action | Backend |
|---|---|
| `list_disks` | `lsblk -J -o ...` |
| `mount_disk` | `udisksctl mount -b <device>` |
| `unmount_disk` | `udisksctl unmount -b <device>` |

- Capability: `udisks2` (lsblk + udisksctl).
- Module switch: `disks`.
- Device must be an absolute `/dev/...` path, no `..`.
- Root commands; no format/delete.

## GUI

- Sidebar **Disks**.
- Device/partition list with size, filesystem, label, mountpoint.
- Mount/unmount buttons.
- Busy/error states.
