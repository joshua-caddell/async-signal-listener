#include <Arduino.h>
#include <sys/time.h>
#include "driver/spi_slave.h"

//#define DEBUG_PRINT 1
#define USE_UART 1

#define PIN_SCK_V   18
#define PIN_MISO_V  19
#define PIN_MOSI_V  23
#define PIN_CS_V    5

// HSPI
#define PIN_SCK   14
#define PIN_MISO  12
#define PIN_MOSI  13
#define PIN_CS    15

// UART
#define UART_RX 16
#define UART_TX 17
#define UART_BAUD 115200

#define BUF_SIZE    16
#define BUF_SIZE_H  24

// HSPI ready ready pin
#define PIN_READY 26

#define INTERVAL_IDX 0
const uint8_t intervals[5] = {11, 17, 23, 29, 37};

const int PPS_PIN = 27; // D27

volatile unsigned long pps_micros = 0;
volatile bool pulse_received = false;
volatile bool synced = false;

WORD_ALIGNED_ATTR uint8_t rxBuf[BUF_SIZE];
WORD_ALIGNED_ATTR uint8_t txBuf[BUF_SIZE];

WORD_ALIGNED_ATTR uint8_t rxBuf_h[BUF_SIZE_H];
WORD_ALIGNED_ATTR uint8_t txBuf_h[BUF_SIZE_H];

void IRAM_ATTR onSetup(spi_slave_transaction_t *t) { gpio_set_level((gpio_num_t)PIN_READY, 1); }
void IRAM_ATTR onTrans(spi_slave_transaction_t *t) { gpio_set_level((gpio_num_t)PIN_READY, 0); }

void generateSample(int64_t timeStampNs) {
    double frequency = 3.0;
    double amplitude = 3.0;
    double t = (double)timeStampNs / 1e9;

    double value = amplitude * sin(2.0 * M_PI * frequency * t);

    uint64_t ts = (uint64_t)timeStampNs;
    memset(txBuf_h, 0, BUF_SIZE_H);
    memcpy(&txBuf_h[0], &value, sizeof(double));
    memcpy(&txBuf_h[8], &ts, sizeof(ts));     // low 8 bytes of the u128; upper 8 stay zero
}

void send_wave_sample(void *pvParameters) {
    #ifdef USE_UART
    Serial2.begin(UART_BAUD, SERIAL_8N1, UART_RX, UART_TX);
    #else
    pinMode(PIN_READY, OUTPUT);
    digitalWrite(PIN_READY, LOW);

    spi_bus_config_t buscfg_h = {};
    buscfg_h.mosi_io_num   = PIN_MOSI;
    buscfg_h.miso_io_num   = PIN_MISO;
    buscfg_h.sclk_io_num   = PIN_SCK;
    buscfg_h.quadwp_io_num = -1;
    buscfg_h.quadhd_io_num = -1;

    spi_slave_interface_config_t slvcfg_h = {};
    slvcfg_h.mode         = 0;
    slvcfg_h.spics_io_num = PIN_CS;
    slvcfg_h.queue_size   = 1;
    slvcfg_h.post_setup_cb = onSetup;
    slvcfg_h.post_trans_cb = onTrans;

    esp_err_t err = spi_slave_initialize(SPI2_HOST, &buscfg_h, &slvcfg_h, SPI_DMA_CH_AUTO);

    if (err != ESP_OK) {
        Serial.printf("HSPI spi_slave_initialize failed: %s\n", esp_err_to_name(err));
        vTaskDelete(NULL);
    }
    #endif

    const TickType_t period = pdMS_TO_TICKS(intervals[INTERVAL_IDX]);
    TickType_t lastWake = xTaskGetTickCount();

    while(1) {

        vTaskDelayUntil(&lastWake, period);

        if (synced) {
            struct timeval tv;
            gettimeofday(&tv, NULL);
            int64_t now_ns = (int64_t)tv.tv_sec * 1000000000LL + (int64_t)tv.tv_usec * 1000LL; //nanos

            generateSample(now_ns);

            #ifdef USE_UART
            size_t n = Serial2.write(txBuf_h, BUF_SIZE_H);
            Serial2.flush();
            if (n != BUF_SIZE_H) {
                Serial.printf("uart short write: %u/%u\n", (unsigned)n, (unsigned)BUF_SIZE_H);
            }
            #else
            spi_slave_transaction_t t = {};
            t.length    = BUF_SIZE_H * 8;   // bits
            t.tx_buffer = txBuf_h;
            t.rx_buffer = rxBuf_h;

            esp_err_t err = spi_slave_transmit(SPI2_HOST, &t, portMAX_DELAY);
            if (err != ESP_OK) {
                Serial.printf("transmit error: %s\n", esp_err_to_name(err));
            }
            #endif
        }
    }
}

void IRAM_ATTR onPPS() {
    pps_micros = micros();
    pulse_received = true;
}

void setup() {
    Serial.begin(115200);

    pinMode(PPS_PIN, INPUT_PULLDOWN);
    attachInterrupt(digitalPinToInterrupt(PPS_PIN), onPPS, RISING);

    spi_bus_config_t buscfg = {};
    buscfg.mosi_io_num   = PIN_MOSI_V;
    buscfg.miso_io_num   = PIN_MISO_V;
    buscfg.sclk_io_num   = PIN_SCK_V;
    buscfg.quadwp_io_num = -1;
    buscfg.quadhd_io_num = -1;

    spi_slave_interface_config_t slvcfg = {};
    slvcfg.mode         = 0;
    slvcfg.spics_io_num = PIN_CS_V;
    slvcfg.queue_size   = 1;

    esp_err_t err = spi_slave_initialize(SPI3_HOST, &buscfg, &slvcfg, SPI_DMA_CH_AUTO);

    if (err != ESP_OK) {
        Serial.printf("spi_slave_initialize failed: %s\n", esp_err_to_name(err));
        while (true) delay(1000);
    }

    for (int i = 0; i < 8; i++) txBuf[i] = (uint8_t)((uint64_t)INT64_MIN >> (i * 8));
    memset(txBuf_h, 0, BUF_SIZE_H);

    xTaskCreatePinnedToCore(send_wave_sample, "send_wave", 4096, NULL, 3, NULL, 0);
}

void loop() {
    unsigned long now_micros;

    if (pulse_received) {
        spi_slave_transaction_t t;
        memset(&t, 0, sizeof(t));
        t.length = BUF_SIZE * 8;
        t.tx_buffer = txBuf;
        t.rx_buffer = rxBuf;

        esp_err_t ret = spi_slave_transmit(SPI3_HOST, &t, portMAX_DELAY);

        if (ret == ESP_OK) {
            now_micros = micros();
            struct timeval old_tv;
            gettimeofday(&old_tv, NULL);   // ESP32's own time at the same instant as now_micros

            uint64_t true_unix_nanos = 0;
            for (int i = 0; i < 8; i++) {
                true_unix_nanos |= ((uint64_t)rxBuf[i] << (i * 8));
            }

            unsigned long latency_us = now_micros - pps_micros;

            if (latency_us > 500000UL) {   // Pi transfers ~100 ms after the edge; anything this late is a mismatched pair
                Serial.printf("rejected sync: latency %lu us\n", latency_us);
                pulse_received = false;
                for (int i = 0; i < 8; i++) txBuf[i] = (uint8_t)((uint64_t)INT64_MIN >> (i * 8));
                return;
            }

            // time at the PPS edge (in µs) + time elapsed since the edge
            uint64_t now_unix_us = (true_unix_nanos / 1000ULL) + latency_us;

            struct timeval tv;
            tv.tv_sec  = (time_t)(now_unix_us / 1000000ULL);
            tv.tv_usec = (suseconds_t)(now_unix_us % 1000000ULL);

            settimeofday(&tv, NULL);

            int64_t old_unix_us   = (int64_t)old_tv.tv_sec * 1000000LL + old_tv.tv_usec;
            int64_t correction_us = (int64_t)now_unix_us - old_unix_us;  // + means the ESP32 was behind
            for (int i = 0; i < 8; i++) {
                txBuf[i] = (uint8_t)((uint64_t)correction_us >> (i * 8));
            }

            if (correction_us < 40) {
                synced = true;
            }
            

            #ifdef DEBUG_PRINT
            Serial.printf("Correction: %lld us.\n", correction_us);
            #endif
        } else {
            Serial.printf("transmit error: %s\n", esp_err_to_name(ret));
        }

        pulse_received = false;
    }
}



