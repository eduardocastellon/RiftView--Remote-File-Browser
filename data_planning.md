# Data planning

How RiftView's data is structured.

- Relational data is stored in SQLite: `riftview.db` in the data folder.
- Actual schema: [`src/database/migrations/001_devices_and_shares.sql`](src/database/migrations/001_devices_and_shares.sql)
- Times are Unix epoch milliseconds (integer). `null` = not set.

Not in the database (kept as files in the data folder):

| File | Holds |
|---|---|
| `config.json` | Server settings |
| `admin_token` | Secret for the host-only admin API |
| `tls/` | Self-signed certificate + private key |

## `devices`

General info about each paired device.

| Column | Type | Notes |
|---|---|---|
| `id` | string, **primary key** | e.g. `dev_3f2a…`, also the ID the app uses to log in |
| `name` | string | Name the device gave itself at pairing, e.g. "Lalo's iPhone" |
| `custom_alias` | string \| null | Custom name the host gave the device (shown instead of `name`) |
| `public_key` | string | Ed25519 public key (base64), cryptographic ID to authenticate the device |
| `key_version` | int | Identifies the device credential/key version, starts at 1 |
| `device_type` | string | `phone`, `tablet`, `laptop`, `desktop`, `other` |
| `platform` | string | `ios`, `android`, `macos`, `windows`, `linux`, `other` |
| `app_version` | string | e.g. `1.4.0`, sent at pairing, updated at each login |
| `paired_at_ms` | int | When the device first paired |
| `last_connected_at_ms` | int \| null | Last successful login |
| `revoked_at_ms` | int \| null | `null` = trusted device; set = revoked forever (row kept as a record) |

## `device_permissions`

One row per permission a device has. No row = not allowed.

| Column | Type | Notes |
|---|---|---|
| `device_id` | string | Refers to `devices.id` (rows are deleted with the device) |
| `permission` | string | `browse`, `read`, `upload`, `rename`, `move`, `delete` |

Primary key: (`device_id`, `permission`)

| Permission | Allows |
|---|---|
| `browse` | List shares/folders and see file details |
| `read` | Download files |
| `upload`, `rename`, `move`, `delete` | Changing files; only works in shares marked `writable` |

## `shares`

Folders the host chose to share.

| Column | Type | Notes |
|---|---|---|
| `id` | string, **primary key** | e.g. `shr_3f2a…` |
| `name` | string | Name devices see |
| `path` | string, **unique** | Real folder path on the host, never sent to devices |
| `writable` | bool | `false` = no device can change anything here, whatever its permissions |
| `created_at_ms` | int | |
