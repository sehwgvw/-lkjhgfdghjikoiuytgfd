@echo off
rem Запуск бота Alyaska Garant. Перед стартом закрывает возможные прошлые
rem копии: две копии одного бота несовместимы — Telegram отдаёт обновления
rem только одному процессу, второй получает "terminated by other getUpdates".
cd /d "%~dp0"

taskkill /IM alyaska_garant.exe /F >nul 2>&1

if not exist "target\release\alyaska_garant.exe" (
    echo Сборка бота...
    cargo build --release
    if errorlevel 1 exit /b 1
)

echo Запуск бота, лог: bot.log
start "Alyaska Garant" /min cmd /c "target\release\alyaska_garant.exe >> bot.log 2>&1"
timeout /t 5 /nobreak >nul
echo Бот запущен. Остановить: stop_bot.bat
