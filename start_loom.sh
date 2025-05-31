#!/bin/sh

# Start loom_backrun in background with full path
/app/loom_backrun &

# Start loom_base in background with full path
/app/loom_base &

# Wait indefinitely to keep container running
wait
