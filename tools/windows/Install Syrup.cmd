@echo off
rem Sets up Syrup's brain for the phone app on this PC: it runs from now on,
rem starts by itself whenever you sign in, and Tailscale Funnel gives it an
rem address your phone can reach. Run it again after updating Syrup.
title Install Syrup
cd /d "%~dp0"
syrup.exe install
echo.
pause
