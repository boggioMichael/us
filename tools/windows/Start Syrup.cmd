@echo off
rem Syrup's brain for the phone app. Keep this window open while you play.
rem It listens only on this computer; Tailscale Funnel carries the phone to it
rem (tailscale funnel --bg 8080). See docs/iphone.md.
title Syrup
cd /d "%~dp0"
echo Syrup is listening for your phone. Keep this window open while you play.
echo.
syrup.exe serve --bind 127.0.0.1 --port 8080 %*
echo.
echo Syrup stopped.
pause
