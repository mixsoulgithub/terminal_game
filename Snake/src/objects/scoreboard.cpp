#include "scoreboard.hpp"

ScoreBoard::ScoreBoard():Object(Outlook(std::string(" "),1)),score(0),level(1),lines_cleared(0){
}

int ScoreBoard::flush(){
    return 0;
}
int ScoreBoard::flush(int x, int y){
    return 0;
}
