#!/bin/bash
# =============================================================================
# zenv Release Script
# =============================================================================
# Automates version updates across the codebase.
#
# Usage:
#   ./scripts/release.sh <version> [description]
#
# Examples:
#   ./scripts/release.sh 0.3.7 "Performance improvements"
#   ./scripts/release.sh 0.4.0 "Type generation feature"
#
# What it updates:
#   - Cargo.toml (version field)
#   - .github/actions/zenv-action/action.yml (fallback VERSION)
#   - todo.md (Current Version header)
#
# What you do manually:
#   - Add CHANGELOG.md entry
#   - Add todo.md version history row
#   - git commit, tag, push
# =============================================================================

set -e

VERSION=$1
DESC=$2
DATE=$(date +%Y-%m-%d)

# Color codes
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Validate input
if [ -z "$VERSION" ]; then
    echo -e "${RED}Error: Version required${NC}"
    echo ""
    echo "Usage: ./scripts/release.sh <version> [description]"
    echo ""
    echo "Examples:"
    echo "  ./scripts/release.sh 0.3.7 \"Performance improvements\""
    echo "  ./scripts/release.sh 0.4.0 \"Type generation feature\""
    exit 1
fi

# Validate version format (semver)
if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?$ ]]; then
    echo -e "${RED}Error: Invalid version format${NC}"
    echo "Expected semver format: X.Y.Z or X.Y.Z-prerelease"
    exit 1
fi

echo "Updating to v$VERSION..."
echo ""

# 1. Update Cargo.toml
if [ -f "Cargo.toml" ]; then
    sed -i "s/^version = \".*\"/version = \"$VERSION\"/" Cargo.toml
    echo -e "${GREEN}[OK]${NC} Cargo.toml"
else
    echo -e "${RED}[ERROR]${NC} Cargo.toml not found"
    exit 1
fi

# 2. Update action.yml fallback
ACTION_FILE=".github/actions/zenv-action/action.yml"
if [ -f "$ACTION_FILE" ]; then
    sed -i "s/VERSION=\"[^\"]*\"/VERSION=\"$VERSION\"/" "$ACTION_FILE"
    echo -e "${GREEN}[OK]${NC} action.yml fallback"
else
    echo -e "${YELLOW}[SKIP]${NC} action.yml not found"
fi

# 3. Update todo.md header
if [ -f "todo.md" ]; then
    sed -i "s/\*\*Current Version:\*\* .*/\*\*Current Version:\*\* $VERSION/" todo.md
    echo -e "${GREEN}[OK]${NC} todo.md header"
else
    echo -e "${YELLOW}[SKIP]${NC} todo.md not found"
fi

echo ""
echo -e "${GREEN}Version updated to $VERSION${NC}"
echo ""
echo "========================================="
echo "MANUAL STEPS REMAINING:"
echo "========================================="
echo ""
echo "1. Add to CHANGELOG.md:"
echo "   ----------------------------------------"
echo "   ## [$VERSION] - $DATE"
echo ""
echo "   ### Added/Changed/Fixed"
if [ -n "$DESC" ]; then
    echo "   - $DESC"
else
    echo "   - <describe changes here>"
fi
echo "   ----------------------------------------"
echo ""
echo "2. Add to todo.md version history table:"
echo "   | v$VERSION | $DATE | ${DESC:-<description>} |"
echo ""
echo "3. Git commands:"
echo "   git add -A"
echo "   git commit -m \"Release v$VERSION: ${DESC:-<description>}\""
echo "   git tag v$VERSION"
echo "   git push origin main --tags"
echo ""
echo "4. Trusted Publishing will auto-publish to crates.io"
echo ""
