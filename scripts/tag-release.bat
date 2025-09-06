@echo off
setlocal enabledelayedexpansion

REM Simple helper to tag and push a release.
REM Usage: scripts\tag-release.bat 1.2.3

if "%~1"=="" (
  echo Usage: %~nx0 VERSION
  echo Example: %~nx0 1.2.3
  exit /b 1
)

set "VERSION=%~1"

echo Preparing to create tag v%VERSION% ...

REM Ensure working tree is clean
git diff --quiet || (
  echo ^> Working tree has unstaged changes. Commit or stash first.
  exit /b 1
)
git diff --cached --quiet || (
  echo ^> There are staged but uncommitted changes. Commit first.
  exit /b 1
)

REM Fetch tags and verify tag does not already exist
git fetch --tags >nul 2>&1
git rev-parse "v%VERSION%" >nul 2>&1 && (
  echo ^> Tag v%VERSION% already exists. Choose a new version.
  exit /b 1
)

REM Create annotated tag and push
git tag -a "v%VERSION%" -m "Release v%VERSION%" || (
  echo ^> Failed to create tag.
  exit /b 1
)

git push origin "v%VERSION%" || (
  echo ^> Failed to push tag to origin.
  exit /b 1
)

echo Done. Pushed tag v%VERSION%.
echo GitHub Actions will build binaries and create the release.

endlocal

