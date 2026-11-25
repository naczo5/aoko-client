---
title: Accounts
description: Import logins, switch the session the game runs, and restore the launch session.
---

The **Accounts** tab in the external GUI holds saved logins and switches the session the running game uses.

## Import

**Import file** takes one or more files, and **Paste** takes the same text as a string. Both accept:

- Localts MSA tokens (`M.C...`)
- Netscape `cookies.txt` exports
- Cookie header strings
- Minecraft access tokens (bare, `Bearer`, or JSON-wrapped)

**Add offline** saves a cracked username of 3 to 16 letters, numbers, or underscores.

Dropping several files on the tab imports them in order with a two second gap, so each authentication chain finishes before the next one starts. Folders are rejected. Drop the file itself.

## Switch

Saved accounts show as cards. The card that matches the account the game is signed in with carries an accent dot.

- **Use account** signs the game in as the selected account.
- **Restore launch account** puts back the session the client started with.
- **Delete** removes the saved credentials from this machine.
- **Refresh** reads the account the game is signed in with now.

The strip above the cards reads **Bridge offline**, **At main menu**, or **In a world**.

## Requirements

- The client must be injected and the bridge connected. Otherwise the strip reads **Bridge offline** and the actions stay disabled.
- Switching and restoring need the game at the main menu. In a world the game refuses the request and the status line asks you to return to the menu first.
- A bridge built without the account actions reports that this build cannot switch accounts. Update aoko, then reconnect.

## Stored credentials

Saved credentials are written to `%AppData%\Aoko\accounts.json` unencrypted. Any program running as you, or anyone who can open your user profile, can read the tokens in that file. **Open folder** in the footer opens the directory that holds it.

An optional checkbox writes saved Microsoft accounts into Lunar Client's own `accounts.json` after an import or switch. It is off by default, offline accounts are skipped, and Lunar reads that file only at startup.

## Limits

- Imports run one file at a time with a two second gap, so a long drop takes a while.
- A Localts, cookie, or token account re-authenticates when you use it, so an online switch needs a working connection. A cracked username does not.
