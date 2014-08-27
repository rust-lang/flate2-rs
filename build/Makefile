ifneq ($(findstring mingw,$(TARGET)),)
	CC := gcc
endif

ifneq ($(findstring i686,$(TARGET)),)
	CC := $(CC) -m32
else
	CC := $(CC) -m64 -fPIC
endif

all:
	$(CC) $(CFLAGS) -O2 miniz.c -c -o $(OUT_DIR)/miniz.o
	$(AR) crus $(OUT_DIR)/libminiz.a $(OUT_DIR)/miniz.o
