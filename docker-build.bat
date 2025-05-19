@echo off
echo Building Docker image...
docker build -t loom-base:latest .

if %ERRORLEVEL% EQU 0 (
  echo Docker image built successfully!
  echo To run the container:
  echo   docker run -d --name loom-base -v %cd%\config.toml:/app/config.toml loom-base:latest
  echo.
  echo To use docker-compose:
  echo   docker-compose up -d
) else (
  echo Failed to build Docker image.
  exit /b 1
)