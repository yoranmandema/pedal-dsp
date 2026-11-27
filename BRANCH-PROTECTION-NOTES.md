# Branch protection fix needed:
# The GitHub API returns 404 for branch protection PATCH requests using the
# fine-grained PAT (github_pat_...). This is likely because the token lacks
# the "administration" scope. To fix this:
#
# 1. Go to GitHub Settings > Developer settings > Personal access tokens
# 2. Create a classic PAT with admin:org scope (or use GitHub App)
# 3. Update the GITHUB_TOKEN in workflows to use the new PAT
#
# Alternatively, the repo owner can disable "Require code owner reviews" via
# GitHub UI: Settings > Branches > Edit protection > uncheck "Require code owner reviews"

# Current branch protection for develop:
# - required_status_checks: cargo test, UI build
# - required_approving_review_count: 1
# - require_code_owner_reviews: true (BLOCKING if no CODEOWNERS file exists)

# Note: Since there is no CODEOWNERS file, code owner reviews requirement is
# effectively a no-op, but GitHub still requires 1 approving review for merge.
# The auto-merge.yml already handles this with --auto flag.
