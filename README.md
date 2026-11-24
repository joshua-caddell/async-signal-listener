# esp32-pps
This the code that ran on each ESP-32. It has code to sync the clock to a time server and code that sends a sine wave data over SPI or UART.

# gpiod-reactor
This is a lightweight reactor to receive SPI data when a given GPIO pin receives a HIGH signal. This was used to gather synchronous timing data for SPI performance.

# signal-sample
This is application code that implements the SignalEventHandler trait in tokio-signal.

# spi-pps
This is the time synchronization code that runs on the time server. It sends a timestamp to the conected ESP-32s.

# tokio-gpiod
This is not my code. There was a bug in this library and the developer is not responding to pull requests. I clone this library and made the bug fix here.

# tokio-signal
This is my SignalListener library.

# tokio-spi-rx
This is the application that I used to collected timing data. It actually works for UART and SPI controllers.

# uart-reactor
This is a lightweight reactor to collect timing analysis for synchronous UART processing. 