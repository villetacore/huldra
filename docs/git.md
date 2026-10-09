# Git

Huldra has its own `git`. It works with real git repositories and
servers: you can clone from GitHub, commit, and push back over HTTPS.

```text
root@huldra:~# git clone --depth 1 https://github.com/villetacore/huldra.git
Cloning into 'huldra'...
Received 658 objects, 882 KiB in 6.7s
 * [new branch]      main -> origin/main
Checked out 'main' (457 files)
root@huldra:~# cd huldra && echo "hello" > notes.txt
root@huldra:~/huldra# git add notes.txt && git commit -m "notes"
[main 3f2a9c1] notes
 1 file changed
root@huldra:~/huldra# git push https://USER:TOKEN@github.com/USER/huldra.git
```

## Commands

| command | |
|---|---|
| `git init [DIR]` | create a repository (branch `main`) |
| `git clone [--depth N] URL [DIR]` | copy a repository; `--depth 1` fetches only the latest commit |
| `git status [-s]` | branch, ahead/behind, staged, unstaged and untracked files |
| `git add PATH... \| -A` | stage files (`.` for everything under the current directory) |
| `git rm [--cached] PATH...` | remove files from the index (and the disk) |
| `git restore [--staged] PATH...` | undo changes to files, or unstage them |
| `git commit [-a] [-m MSG]` | record the staged changes; without `-m` the editor opens |
| `git log [--oneline] [-n N] [REV]` | history, with branch and tag labels |
| `git diff [--cached] [PATH...]` | unstaged (or staged) changes as a patch |
| `git show [REV]` | a commit and its patch |
| `git branch [-a] [-d NAME] [NAME [REV]]` | list, create or delete branches |
| `git checkout [-b] BRANCH\|REV`, `git switch [-c] BRANCH` | switch branches; a remote branch becomes a local tracking branch |
| `git checkout -- PATH` | restore a file from the index |
| `git reset [--hard] [REV]` | move the branch; `--hard` also resets the files |
| `git fetch [REMOTE]` | download new commits into `origin/*` |
| `git pull` | fetch, then fast-forward the current branch |
| `git push [-f] [REMOTE [BRANCH]]` | send commits; refuses non-fast-forward updates without `-f` |
| `git tag [-d] [NAME [REV]]` | lightweight tags |
| `git remote [-v]`, `git remote add NAME URL` | remotes |
| `git config [--global] KEY [VALUE]` | `user.name`, `user.email`, … |
| `git rev-parse REV`, `git cat-file -p\|-t\|-s REV` | plumbing |

Revisions can be branch names, tags, `origin/main`, full or abbreviated
ids, `HEAD~2` and `main^`.

## Setting up

```sh
git config --global user.name "Your Name"
git config --global user.email you@example.com
```

Without them, commits are signed `root <root@HOSTNAME>`.

## Authentication

Cloning public repositories needs nothing. To push to GitHub (or any
server that wants a password), use a personal access token. Either put it
in the URL:

```sh
git remote set-url origin https://USER:TOKEN@github.com/USER/REPO.git
```

or in `~/.git-credentials`, one URL per line, which is the format git's
`store` helper uses:

```text
https://USER:TOKEN@github.com
```

HTTPS certificates are checked against `/etc/ssl/certs`.
`GIT_SSL_NO_VERIFY=1` turns the check off.

## How it works

The logic lives in [`libs/git`](../libs/git) and does no I/O, so it is
tested on the build machine against data made by real git:

- **Objects**: blobs, trees (in git's sort order), commits and tags,
  named by SHA-1 and stored as zlib-compressed loose objects. SHA-1 comes
  from `libs/crypto` and zlib from `libs/flate`.
- **Packs**: what a server sends is a pack file. Huldra resolves its
  deltas (both `ofs-delta` and `ref-delta`) and writes the `.idx`, which
  comes out byte for byte the same as the one git writes. Objects are then
  read straight from the pack.
- **The index** (`.git/index`): versions 2 and 3 are read, version 2 is
  written. Stat data lets `status` skip files that did not change.
- **Smart HTTP**: reference advertisement, `git-upload-pack` with
  side-band progress and shallow clones, `git-receive-pack` with
  `report-status`. Pushes send a pack of whole objects.
- **Diffs**: Myers' algorithm, unified output with three lines of
  context, the same hunks as `git diff`.

The program is in [`user/src/bin/git`](../user/src/bin/git): `repo.rs`
(object database, refs, config), `worktree.rs` (index ↔ files, checkout,
status), `remote.rs` (HTTP), `main.rs` (commands).

`cargo xtask test` clones from a real `git http-backend` on the host,
over HTTP and HTTPS. It commits, pushes, branches and makes a shallow
clone, and then the host runs `git fsck --strict` on what Huldra pushed.

## Not yet

- Merging: `pull` only fast-forwards. On diverged branches, commit your
  work elsewhere or use `git reset --hard origin/BRANCH`.
- Stash, rebase, submodules (shown as empty directories), the SSH and
  `git://` transports, and dumb HTTP servers.
- Packs are held in memory, so very large repositories need `--depth 1`.
