// reference: decode an MP3 with minimp3's mp3dec_load_buf (float output, no SIMD), write
// "ch rate" then interleaved floats; "ERR" when no audio is found, "MEM" when minimp3 gives up
// on a frame count too big to allocate
#define MINIMP3_IMPLEMENTATION
#define MINIMP3_NO_SIMD
#define MINIMP3_FLOAT_OUTPUT
#include "minimp3_ex.h"
#include <stdio.h>
#include <stdlib.h>
int main(int argc, char **argv) {
  FILE *f = fopen(argv[1], "rb"); if (!f) return 2;
  fseek(f, 0, SEEK_END); long n = ftell(f); fseek(f, 0, SEEK_SET);
  unsigned char *d = malloc(n ? n : 1); if (fread(d, 1, n, f) != (size_t)n) return 2; fclose(f);
  FILE *o = fopen(argv[2], "wb");
  int rounds = argc > 3 ? atoi(argv[3]) : 1;
  for (int r = 0; r < rounds; r++) {
    mp3dec_t dec; mp3dec_file_info_t info;
    int ret = mp3dec_load_buf(&dec, d, n, &info, NULL, NULL);
    if (ret == MP3D_E_MEMORY) { fprintf(o, "MEM\n"); break; }
    if (!info.hz) { fprintf(o, "ERR\n"); break; }
    if (!r) fprintf(o, "%d %d\n", info.channels, info.hz);
    fwrite(info.buffer, 4, info.samples, o);
    free(info.buffer);
  }
  fclose(o); free(d); return 0;
}
