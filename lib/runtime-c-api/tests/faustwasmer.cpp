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
#include "faust/gui/httpdUI.h"
#include "faust/gui/SoundUI.h"
#include "faust/misc.h"

list<GUI*> GUI::fGuiList;
ztimedmap GUI::gTimedZoneMap;

int main(int argc, char* argv[])
{
    if (isopt(argv, "-h") || isopt(argv, "-help")) {
        cout << "faustwasmer foo.wasm" << endl;
        cout << "Open the http://127.0.0.1:5510 URL to get an http based control\n";
        exit(EXIT_FAILURE);
    }
    
    wasmer_dsp_factory factory(argv[1]);
    dsp* DSP = factory.createDSPInstance();
    
    jackaudio audio;
    if (!audio.init(argv[1], DSP)) {
        return 0;
    }
    
    httpdUI httpdinterface(argv[1], DSP->getNumInputs(), DSP->getNumOutputs(), argc, argv);
    DSP->buildUserInterface(&httpdinterface);
    
    SoundUI soundui;
    DSP->buildUserInterface(&soundui);
    
    audio.start();
    
    httpdinterface.run();
    
    char c;
    while ((c = getchar()) != 'q') {
        usleep(1000000);
    }
    
    audio.stop();
    return 0;
}
