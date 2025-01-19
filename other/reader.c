#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define L0 0
#define L1 1
#define R0 2
#define R1 3
#define L2 4
#define L3 5
#define R2 6
#define R3 7
#define L4 8
#define L5 9
#define R4 10
#define R5 11
#define L6 12
#define L7 13
#define R6 14
#define R7 15

#define SKIP_CLOCK_BYTES 3072000
#define MAX_FILE_SIZE 33554432

/*typedef uint32_t line[4];*/
/*typedef uint32_t channels[16];*/

/*typedef struct {*/
/*  uint32_t sample[4];*/
/*} line;*/

typedef struct {
  /*uint32_t& operator[](int i) { return sample[i]; }*/
  uint32_t sample[16];
} channels;

typedef struct {
  channels c[33];
  uint8_t i;
  uint8_t ci;
} channel_buffer;

int load_file(const char *path, uint32_t buffer[MAX_FILE_SIZE], size_t *size) {
  FILE *f = fopen(path, "rb");
  if (!f) {
    fprintf(stderr, "Failed to open file: %s", path);
    fclose(f);
    return 1;
  }

  fseek(f, 0L, SEEK_END);
  *size = ftell(f);
  fseek(f, 0L, SEEK_SET);
  if (*size > MAX_FILE_SIZE) {
    fprintf(stderr, "File size too big: %zu bytes. %d bytes allowed.", *size,
            MAX_FILE_SIZE);
    fclose(f);
    return 2;
  }

  if (*size % 4 != 0) {
    fprintf(stderr, "Number of bytes is not a multiple of 4");
    fclose(f);
    return 3;
  }

  *size /= 4;

  fread(buffer, *size, 1, f);
  fclose(f);

  buffer[*size] = 0;

  return 0;
}

int process_buffer(uint32_t *buffer, size_t size, size_t i, channel_buffer *cb,
                   bool start) {
  size_t samples_processed = 0;

  if (start) {
    // skip to start
    while (i < size - 2 && (((buffer[i + 0] >> 24) & 0x07) != 0 ||
                            ((buffer[i + 1] >> 24) & 0x07) != 1 ||
                            ((buffer[i + 2] >> 24) & 0x07) != 0)) {
      i++;
    }
    // load first 33 sets of samples
    while (i < size - 1 && samples_processed < 33) {
      if (buffer[i] == 0xeeeeeeee && buffer[i + 1] == 0xeeeeeeee) {
        i += 4;
      } else {
        cb->c[cb->i].sample[cb->ci] = buffer[i];
        if (cb->ci == 15) {
          cb->ci = 0;
          cb->i = (cb->i + 1) % 33;
          samples_processed++;
        }
      }
    }
  }

  while (i < size - 1) {
    if (buffer[i] == 0xeeeeeeee && buffer[i + 1] == 0xeeeeeeee) {
      i += 4;
    } else {
      cb->c[cb->i].sample[cb->ci] = buffer[i];
      if (cb->ci == 15) {
        // TODO: calculate directional results
        cb->ci = 0;
        cb->i = (cb->i + 1) % 33;
      } else {
        cb->ci++;
      }
    }
  }

  return 0;
}

int write_channels_to_files(uint32_t *buffer, size_t size, size_t i) {
  // skip to start
  while (i < size - 2 && (((buffer[i + 0] >> 24) & 0x07) != 0 ||
                          ((buffer[i + 1] >> 24) & 0x07) != 1 ||
                          ((buffer[i + 2] >> 24) & 0x07) != 0)) {
    i++;
  }

  uint32_t c[16][MAX_FILE_SIZE / 4 + 1];
  size_t ci[16];
  size_t j = 0;

  while (i < size - 1) {
    if (buffer[i] == 0xeeeeeeee && buffer[i + 1] == 0xeeeeeeee) {
      i += 4;
    } else {
      c[j][ci[j]++] = buffer[i];
      j = (j + 1) % 16;
    }
  }

  for (i = 0; i < 16; i++) {
    char buf[7];
    snprintf(buf, 7, "test%zu", i);
    FILE *f = fopen(buf, "wb");
    fwrite(c[i], sizeof(uint32_t), ci[i], f);
    fclose(f);
  }

  return 0;
}

int main(int argc, char *argv[]) {
  size_t size;
  uint8_t buffer[MAX_FILE_SIZE];

  load_file(argv[1], buffer, &size);

  return EXIT_SUCCESS;
}
