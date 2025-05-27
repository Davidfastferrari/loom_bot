#!/bin/sh

# Start loom_backrun in background
/app/loom_backrun &

# Start loom_base in background
/app/loom_base &

# Wait indefinitely to keep container running
wait
