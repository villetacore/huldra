"""Regenerates the fixtures with real git:

    python libs/git/testdata/generate.py

repo.pack/.idx     a repository packed with ofs-deltas (git repack -adf)
refdelta.pack      the same objects with ref-deltas (pack-objects)
objects.txt        id, type and size of every object
index.bin          .git/index after `git add`, with HEAD.txt
diff-a.txt/-b.txt  two versions of a file, and diff.txt: `git diff` hunks
"""
import os, shutil, subprocess, tempfile

here = os.path.dirname(os.path.abspath(__file__))
env = dict(os.environ, GIT_AUTHOR_NAME="Huldra Test", GIT_AUTHOR_EMAIL="test@huldra", GIT_COMMITTER_NAME="Huldra Test",
           GIT_COMMITTER_EMAIL="test@huldra", GIT_AUTHOR_DATE="1700000000 +0100", GIT_COMMITTER_DATE="1700000000 +0100")

def git(*args, cwd, inp=None):
    return subprocess.run(["git", "-c", "core.autocrlf=false", "-c", "init.defaultBranch=main", *args], cwd=cwd, env=env,
                          input=inp, capture_output=True, check=True).stdout

tmp = tempfile.mkdtemp()
git("init", "-q", ".", cwd=tmp)
lines = ["line %d of a long text file that will change a little each commit\n" % i for i in range(400)]
os.makedirs(os.path.join(tmp, "src", "deep"))
for n in range(6):
    lines[n * 37] = "changed in commit %d\n" % n
    open(os.path.join(tmp, "big.txt"), "w", newline="\n").write("".join(lines))
    open(os.path.join(tmp, "src", "deep", "f%d.c" % n), "w", newline="\n").write("int f%d(void) { return %d; }\n" % (n, n))
    open(os.path.join(tmp, "data.bin"), "wb").write(bytes(range(256)) * (n + 1))
    git("add", "-A", cwd=tmp)
    git("commit", "-q", "-m", "commit %d\n\nwith a body line" % n, cwd=tmp)
git("tag", "-a", "v1", "-m", "a tag", cwd=tmp)
git("repack", "-adfq", "--depth=50", cwd=tmp)
packdir = os.path.join(tmp, ".git", "objects", "pack")
for f in os.listdir(packdir):
    if f.endswith(".pack"):
        shutil.copy(os.path.join(packdir, f), os.path.join(here, "repo.pack"))
    if f.endswith(".idx"):
        shutil.copy(os.path.join(packdir, f), os.path.join(here, "repo.idx"))
objs = git("cat-file", "--batch-check", "--batch-all-objects", cwd=tmp)
open(os.path.join(here, "objects.txt"), "wb").write(objs)
ids = b"".join(l.split()[0] + b"\n" for l in objs.splitlines())
ref = git("pack-objects", "--stdout", "-q", "--window=10", cwd=tmp, inp=ids)
open(os.path.join(here, "refdelta.pack"), "wb").write(ref)
shutil.copy(os.path.join(tmp, ".git", "index"), os.path.join(here, "index.bin"))
open(os.path.join(here, "HEAD.txt"), "wb").write(git("rev-parse", "HEAD", "HEAD^{tree}", cwd=tmp) + git("cat-file", "-p", "HEAD", cwd=tmp))

a = "".join("row %d\n" % i for i in range(60))
b = a.replace("row 5\n", "row five\n").replace("row 30\n", "").replace("row 58\n", "row 58\nadded\n")
open(os.path.join(here, "diff-a.txt"), "w", newline="\n").write(a)
open(os.path.join(here, "diff-b.txt"), "w", newline="\n").write(b)
p = subprocess.run(["git", "diff", "--no-index", "--no-indent-heuristic", "-U3", "diff-a.txt", "diff-b.txt"], cwd=here, capture_output=True)
hunks = p.stdout.decode()
open(os.path.join(here, "diff.txt"), "w", newline="\n").write(hunks[hunks.index("@@"):])
shutil.rmtree(tmp, ignore_errors=True)
print("ok")
