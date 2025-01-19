#include <assert.h>
#include <time.h>
/*#include <conio.h>*/
#include <math.h>
/*#include <iostream>*/
#include <stdio.h>
#include <unistd.h>
#include <fcntl.h>
/*#include <windows.h>*/
#include <string.h>

/* run this program using the console pauser or add your own getch, system("pause") or input loop */
char line[256];
unsigned char buffer[64];

int main(int argc, char** argv) {
FILE  * fd_in, * fd_out[16];
int i;
short int j;




if(argc>1){
    strcpy(line, argv[1]);
	 } else {
        printf("Not file name  parameters in command line.....exiting");
        return 1;
}


fd_in= fopen(line,"rb");
if(fd_in == NULL) { fclose(fd_in);
        	printf("\nNO Valid Data file present....exiting",line);
            return 1;
}
printf("\nProcessing Data file = %s\n",line);


fd_out[0]= fopen("ch_1.raw","w+b");
if(fd_out[0] == NULL) { fclose(fd_out[0]);
        	printf("\nI could not open an outpt file 1",line);
            return 1;
}
fd_out[1]= fopen("ch_2.raw","w+b");
if(fd_out[1] == NULL) { fclose(fd_out[1]);
        	printf("\nI could not open an outpt file 2",line);
            return 1;
}
fd_out[2]= fopen("ch_3.raw","w+b");
if(fd_out[2] == NULL) { fclose(fd_out[2]);
        	printf("\nI could not open an outpt file 3",line);
            return 1;
}
fd_out[3]= fopen("ch_4.raw","w+b");
if(fd_out[3] == NULL) { fclose(fd_out[3]);
        	printf("\nI could not open an outpt file 4",line);
            return 1;
}
fd_out[4]= fopen("ch_5.raw","w+b");
if(fd_out[4] == NULL) { fclose(fd_out[4]);
        	printf("\nI could not open an outpt file 5",line);
            return 1;
}
fd_out[5]= fopen("ch_6.raw","w+b");
if(fd_out[5] == NULL) { fclose(fd_out[5]);
        	printf("\nI could not open an outpt file 6",line);
            return 1;
}
fd_out[6]= fopen("ch_7.raw","w+b");
if(fd_out[6] == NULL) { fclose(fd_out[6]);
        	printf("\nI could not open an outpt file 7",line);
            return 1;
}
fd_out[7]= fopen("ch_8.raw","w+b");
if(fd_out[7] == NULL) { fclose(fd_out[7]);
        	printf("\nI could not open an outpt file 8",line);
            return 1;
}
fd_out[8]= fopen("ch_9.raw","w+b");
if(fd_out[8] == NULL) { fclose(fd_out[8]);
        	printf("\nI could not open an outpt file 9",line);
            return 1;
}
fd_out[9]= fopen("ch_10.raw","w+b");
if(fd_out[9] == NULL) { fclose(fd_out[9]);
        	printf("\nI could not open an outpt file 10",line);
            return 1;
}
fd_out[10]= fopen("ch_11.raw","w+b");
if(fd_out[10] == NULL) { fclose(fd_out[10]);
        	printf("\nI could not open an outpt file 11",line);
            return 1;
}
fd_out[11]= fopen("ch_12.raw","w+b");
if(fd_out[11] == NULL) { fclose(fd_out[11]);
        	printf("\nI could not open an outpt file 12",line);
            return 1;
}
fd_out[12]= fopen("ch_13.raw","w+b");
if(fd_out[12] == NULL) { fclose(fd_out[12]);
        	printf("\nI could not open an outpt file 13",line);
            return 1;
}
fd_out[13]= fopen("ch_14.raw","w+b");
if(fd_out[13] == NULL) { fclose(fd_out[13]);
        	printf("\nI could not open an outpt file 14",line);
            return 1;
}
fd_out[14]= fopen("ch_15.raw","w+b");
if(fd_out[14] == NULL) { fclose(fd_out[14]);
        	printf("\nI could not open an outpt file 15",line);
            return 1;
}
fd_out[15]= fopen("ch_16.raw","w+b");
if(fd_out[15] == NULL) { fclose(fd_out[15]);
        	printf("\nI could not open an outpt file 8",line);
            return 1;
}





printf("\nProcessing Out Data file = %s\n",line);
i = fread(&buffer[0],sizeof (unsigned char),45,fd_in);
//i = fread(&buffer[0],sizeof (unsigned char),1,fd_in);
//printf("Track Byte %02x",buffer[0]);
do {
	i = fread(&buffer[0],sizeof (unsigned char),4,fd_in);
	j=0x00000000L;
	if(i==4) {
		//j =  (  ((((unsigned int)buffer[3])<<24)&0xFF0000000L) | ((((unsigned int)buffer[2])<<16)&0xFF0000L) );
		j =  (((((unsigned short)buffer[1])<<8)&0xFF00)  |  (((unsigned short)buffer[0]&0xFF)));
		fwrite(&j,sizeof(short),1,fd_out[buffer[3]&0xf]);
		j =  (((((unsigned short)buffer[2])<<8)&0xFF00)  |  (((unsigned short)buffer[13])&0xFF));
		fwrite(&j,sizeof(short),1,fd_out[buffer[3]&0xf]);
	}
} while (i==4);

fclose(fd_in);
fclose(fd_out[0]);
fclose(fd_out[1]);
fclose(fd_out[2]);
fclose(fd_out[3]);
fclose(fd_out[4]);
fclose(fd_out[5]);
fclose(fd_out[6]);
fclose(fd_out[7]);
fclose(fd_out[8]);
fclose(fd_out[9]);
fclose(fd_out[10]);
fclose(fd_out[11]);
fclose(fd_out[12]);
fclose(fd_out[13]);
fclose(fd_out[14]);
fclose(fd_out[15]);


return 0;

}



