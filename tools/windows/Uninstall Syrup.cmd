@echo off
rem Stops Syrup's brain, and stops it starting with Windows. What it learned stays.
title Uninstall Syrup
cd /d "%~dp0"
syrup.exe uninstall
echo.
pause
