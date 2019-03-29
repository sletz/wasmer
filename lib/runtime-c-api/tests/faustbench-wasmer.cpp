/************************************************************************
 FAUST Architecture File
 Copyright (C) 2019 GRAME, Centre National de Creation Musicale
 ---------------------------------------------------------------------
 This Architecture section is free software; you can redistribute it
 and/or modify it under the terms of the GNU General Public License
 as published by the Free Software Foundation; either version 3 of
 the License, or (at your option) any later version.
 
 This program is distributed in the hope that it will be useful,
 but WITHOUT ANY WARRANTY; without even the implied warranty of
 MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 GNU General Public License for more details.
 
 You should have received a copy of the GNU General Public License
 along with this program; If not, see <http://www.gnu.org/licenses/>.
 
 EXCEPTION : As a special exception, you may create a larger work
 that contains this FAUST architecture section and distribute
 that work under terms of your choice, so long as this FAUST
 architecture section is not modified.
 ************************************************************************/

#include "wasmer_dsp.h"
#include "faust/audio/jack-dsp.h"
#include "faust/dsp/dsp-bench.h"
#include "faust/misc.h"

int main(int argc, char* argv[])
{
    wasmer_dsp* DSP = new wasmer_dsp(argv[1]);
    
    measure_dsp* mes = new measure_dsp(DSP, 512, 5.);  // Buffer_size and duration in sec of  measure
    for (int i = 0; i < 2; i++) {
        mes->measure();
        cout << argv[argc-1] << " : " << mes->getStats() << " " << "(DSP CPU % : " << (mes->getCPULoad() * 100) << ")" << endl;
    }
    
    delete DSP;
    return 0;
}
