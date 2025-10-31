# EWW Monitoring
A prototyle Eww config with real-time monitoring of CPU temperature, core loads, disk usage, network transmit/receive rates and available RAM. 

!!!warning Mostly  generated with AI
## Demo

<video src="https://github.com/user-attachments/assets/c40fdafe-0d8d-482f-8ef1-e899a1a31eb1" autoplay></video>

## Features

- **CPU Usage**: Monitor CPU utilization
- **Memory**: Track RAM and swap usage
- **Disk**: Display disk space and I/O statistics
- **Network**: Show network activity
- **Temperature**: Monitor system temperatures
- **Battery**: Battery status and level
- **Volume**: Audio volume control and display
- **Workspaces**: Current workspace information
- **Time**: Clock and date display

## Components

- **Polling Server**: Rust-based backend (`polling-server/`) that collects system metrics
- **EWW Widgets**: Frontend widgets defined in `eww.yuck` with styling in `eww.scss`

## Installation
Just put it into your config directory and it'll be fine on x86, otherwise rebuild the polling server with ```cargo build```

## Known Issues

- **Thread Usage**: The polling server spawns 60 threads unnecessarily, which may impact system resources.
- **CPU Usage**: 13% of one core is utilized, with all time spent on rendering on bar's site.
If you read this, it's tough to make it optimized, maybe even impossible.
