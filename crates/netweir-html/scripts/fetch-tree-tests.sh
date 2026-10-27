#!/bin/sh
# Fetches the tree-construction tests from web-platform-tests, where they
# moved from html5lib-tests, at a pinned commit. Run it to update them:
#   scripts/fetch-tree-tests.sh COMMIT
set -eu
commit=${1:?usage: scripts/fetch-tree-tests.sh WPT_COMMIT}
dir=tests/tree-construction
rm -rf "$dir" && mkdir -p "$dir"
gh api "repos/web-platform-tests/wpt/contents/html/syntax/parsing/resources?ref=$commit" \
  --jq '.[] | select(.name | endswith(".dat")) | .name' |
while read -r name; do
  curl -fsS "https://raw.githubusercontent.com/web-platform-tests/wpt/$commit/html/syntax/parsing/resources/$name" -o "$dir/$name"
done
curl -fsS "https://raw.githubusercontent.com/web-platform-tests/wpt/$commit/LICENSE.md" -o "$dir/LICENSE.md"
echo "$commit" > "$dir/COMMIT"
