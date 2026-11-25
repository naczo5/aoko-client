---
title: Servers
description: Saved server list with live status, direct connect, and joins through the client.
---

The **Servers** tab keeps a saved server list and connects without leaving the external GUI. Rows report the MOTD, online and maximum players, and latency from Minecraft server list ping. An address given without a port resolves through SRV records first.

## Use it

- **Direct connect** takes `host` or `host:port`, then **Join** connects.
- **Add server** saves a name and an address. The name may stay empty, and the address is used as the label.
- **Join selected** joins the highlighted row.
- Rows ping once when the tab first opens.
- **Ping all** refreshes every row. Clicking a row's status chip pings that server again.
- **Delete** removes the saved row.

## Requirements

- The client must be injected and the bridge connected. Without either, the join button stays disabled and the tab shows the not-connected message.
- The address must be a valid host or `host:port`, at most 256 characters.

## Limits

- Joining goes through the client's own connect path, so the client's account menu takes no part in it.
- Status is a snapshot from the last ping. Ping a row again, or run **Ping all**, to update it.
- A server that does not answer the status ping stays in the list as an error row. Joining it is still available.
