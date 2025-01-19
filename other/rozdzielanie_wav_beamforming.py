import wave
import struct

def extract_channels(input_file, output_prefix):
  """
  Wczytuje plik WAV 32-bit signed, little-endian,
  ekstrahuje informacje o kanale z 4 najmniej znaczących bitów
  i zapisuje 16 oddzielnych plików WAV.

  Args:
    input_file: Ścieżka do pliku wejściowego WAV.
    output_prefix: Prefiks nazwy dla plików wyjściowych.
  """

  # Otwórz plik wejściowy WAV
  with wave.open(input_file, 'rb') as wf:
    num_channels = wf.getnchannels()
    frame_rate = wf.getframerate()
    sample_width = wf.getsampwidth()
    num_frames = wf.getnframes()

    # Sprawdź, czy plik jest w odpowiednim formacie
    if sample_width != 4:
      raise ValueError("Plik wejściowy musi być 32-bitowy.")

    # Utwórz listy dla danych każdego kanału
    channel_data = [[] for _ in range(16)]

    # Przeczytaj dane z pliku
    for _ in range(num_frames):
      frame = wf.readframes(1)
      for channel in range(num_channels):
        # Wyodrębnij wartość 32-bitową
        value = struct.unpack('<i', frame[channel * 4:(channel + 1) * 4])[0]
        # Wyodrębnij informację o kanale z 4 najmniej znaczących bitów
        channel_index = value & 0x0F
        # Dodaj wartość do odpowiedniej listy
        channel_data[channel_index].append(value)

    # Zapisz dane do oddzielnych plików WAV
    for i in range(16):
      output_file = f"{output_prefix}_{i}.wav"
      with wave.open(output_file, 'wb') as outfile:
        outfile.setnchannels(1)  # Mono
        outfile.setsampwidth(4)  # 32-bit
        outfile.setframerate(frame_rate/4)
        outfile.writeframes(struct.pack('<' + 'i' * len(channel_data[i]), *channel_data[i]))

if __name__ == "__main__":
  input_file = "C://Users//Adam//Downloads//test_4bits.wav"  # Zastąp nazwą swojego pliku wejściowego
  output_prefix = "C://Users//Adam//Downloads//o_"
  extract_channels(input_file, output_prefix)
