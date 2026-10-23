import random, os, enum, decimal, uuid, json
import numpy as np
from point import *

def generateRandomString():
    a = uuid.uuid4().hex[:20].upper()
    b = ''.join([i for i in a if not i.isdigit()]) 
    return b

def generatePointArray(width, height):
    firstNumber = random.randrange(1,7) 
    a = pickGetPointArray(firstNumber, width, height)
    number = random.randrange(1,7) 
    newAry = pickGetPointArray(number, width, height)
    b = a + newAry
    #np.concatenate((a, newAry))
    return a


def pickGetPointArray(number, width, height):
    
    #print("array:" + str(number))
    if number == 1:
        return sargentPointA(width, height)
    elif number == 2:
        return sargentPointB(width, height)
    elif number == 3:
        return sargentPointC(width, height)
    elif number == 4:
        ary = sargentPointC(width, height)
        ary2 = sargentPointB(width, height)
        ary3 = sargentPointA(width, height)
        return ary + ary2 + ary3
    elif number == 5:
        return generateGoldenWidthPointArray(width, height)
    elif number == 6:
        randomNumber = random.randrange(1,width) 
        return generateRandomPointArray(width, height, randomNumber)
    elif number == 7:
        return starryNight(width, height)





def generateRandomPointArray(width, height, numOfPoints):
    numOfPoints = random.randrange(1,width) / 10
    array = []
    for r in range(numOfPoints):
        x = random.randrange(1,width)
        y = random.randrange(1,height)
        p = Point(x,y,"#ffffff","a")

        array.append(p)

    clean  = list(dict.fromkeys(array))
    return clean 

def generateGoldenWidthPointArray(width, height):
    numOfPoints = random.randrange(1,width) / 10
    isLeft = random.randrange(1,100)
    golden = width / 1.618
    if isLeft > 50:
        golden = width - golden
    array = []
    for r in range(numOfPoints):
        x = golden
        y = random.randrange(1,height)
        p = Point(x,y,"#ffffff","a")
        array.append(p)

    clean  = list(dict.fromkeys(array))
    return clean 

def generateGoldenHeightPointArray(width, height):
    numOfPoints = random.randrange(1,width) / 10
    isTop = random.randrange(1,100)
    golden = height / 1.618
    if isTop > 50:
        golden = height - golden
    array = []
    for r in range(numOfPoints):
        x = random.randrange(1,width) 
        y = golden
        p = Point(x,y,"#ffffff","a")
        array.append(p)

    clean  = list(dict.fromkeys(array))
    return clean 

def sargentPoints(width, height):
    array = []
    x =  (22*width)/100 
    y =  (34*height)/100 
    p = Point(x,y,"#ffffff","a")
    array.append(p)

    x =  (64*width)/100 
    y =  (52*height)/100 
    p = Point(x,y,"#ffffff","a")
    array.append(p)

    x =  (89*width)/100 
    y =  (48*height)/100 
    p = Point(x,y,"#ffffff","a")
    array.append(p)
    clean  = list(dict.fromkeys(array))
    return clean 



def centerPoints(width, height):
    array = []
    x =  (50*width)/100 
    y =  (50*height)/100 
    p = Point(x,y,"#ffffff","a")
    array.append(p)
    clean  = list(dict.fromkeys(array))
    return clean 

def sargentPointA(width, height):
    array = []
    x =  (64*width)/100 
    y =  (52*height)/100 
    p = Point(x,y,"#ffffff","a")
    array.append(p)
    clean  = list(dict.fromkeys(array))
    return clean 

def sargentPointB(width, height):
    array = []
    x =  (22*width)/100 
    y =  (34*height)/100 
    p = Point(x,y,"#ffffff","a")
    array.append(p)
    clean  = list(dict.fromkeys(array))
    return clean 


def translateJSPoint(x, y, width, height):
    x1 =  (y*width)/100 
    y1 =  (x*height)/100 
    p = Point(x1,y1,"#ffffff","a")
    return p



#{"panel":{"width":1600,"height":1040},"points":[{"x":"0.313","y":"0.385"},{"x":"2.188","y":"0.865"},{"x":"5.625","y":"1.346"},{"x":"8.375","y":"1.827"},{"x":"11.750","y":"1.923"},{"x":"14.750","y":"1.827"},{"x":"18.563","y":"1.538"},{"x":"17.313","y":"8.942"},{"x":"10.813","y":"7.500"},{"x":"7.625","y":"4.904"},{"x":"4.188","y":"6.731"},{"x":"2.688","y":"11.250"},{"x":"1.688","y":"15.769"},{"x":"8.063","y":"17.308"},{"x":"12.063","y":"13.846"},{"x":"14.813","y":"17.596"},{"x":"14.500","y":"22.692"},{"x":"12.875","y":"26.154"},{"x":"9.750","y":"26.538"},{"x":"8.000","y":"28.942"},{"x":"5.813","y":"29.712"},{"x":"2.375","y":"28.750"},{"x":"0.813","y":"30.000"},{"x":"22.313","y":"30.673"},{"x":"28.063","y":"28.269"},{"x":"24.438","y":"18.462"},{"x":"24.688","y":"8.942"},{"x":"26.813","y":"4.038"},{"x":"35.000","y":"6.154"},{"x":"31.688","y":"14.712"},{"x":"29.000","y":"25.481"},{"x":"25.875","y":"34.038"},{"x":"22.438","y":"35.385"},{"x":"28.875","y":"36.442"},{"x":"35.750","y":"34.038"},{"x":"42.000","y":"31.827"},{"x":"39.750","y":"24.808"},{"x":"46.375","y":"20.962"},{"x":"46.813","y":"27.885"},{"x":"51.750","y":"25.192"},{"x":"48.688","y":"11.923"},{"x":"44.125","y":"15.769"},{"x":"38.688","y":"6.538"},{"x":"34.750","y":"3.269"},{"x":"44.563","y":"2.308"},{"x":"50.187","y":"8.654"},{"x":"48.000","y":"14.904"},{"x":"52.750","y":"21.346"},{"x":"57.813","y":"18.558"},{"x":"61.375","y":"22.308"},{"x":"64.188","y":"25.192"},{"x":"67.125","y":"22.115"},{"x":"67.500","y":"10.577"},{"x":"63.250","y":"14.519"},{"x":"57.500","y":"14.808"},{"x":"56.313","y":"5.962"},{"x":"49.625","y":"2.212"},{"x":"41.688","y":"2.212"},{"x":"68.063","y":"3.750"},{"x":"62.125","y":"1.635"},{"x":"67.500","y":"11.154"},{"x":"67.813","y":"17.212"},{"x":"65.313","y":"18.942"},{"x":"71.313","y":"14.712"},{"x":"76.063","y":"12.596"},{"x":"75.438","y":"7.500"},{"x":"72.188","y":"2.885"},{"x":"78.000","y":"2.019"},{"x":"78.563","y":"8.558"},{"x":"81.125","y":"4.423"},{"x":"81.250","y":"1.346"},{"x":"84.188","y":"1.250"},{"x":"86.438","y":"1.250"},{"x":"63.625","y":"11.923"},{"x":"61.687","y":"20.096"},{"x":"51.750","y":"21.154"},{"x":"47.375","y":"9.231"},{"x":"30.813","y":"10.288"},{"x":"32.313","y":"26.250"},{"x":"25.813","y":"26.346"}]}



def sargentPointC(width, height):
    array = []
    x =  (89*width)/100 
    y =  (48*height)/100 
    p = Point(x,y,"#ffffff","a")
    array.append(p)
    clean  = list(dict.fromkeys(array))
    return clean 


def generatePointsInABox(x1,y1,x2,y2,x3,y3,x4,y4):
    numOfPoints = random.randrange(1,500) 
    array = []

    i = 0
    while i < numOfPoints:
        if x1 < x2:
            x =  random.randrange(x1,x2)
        else:
            x =  random.randrange(x2,x1)
        
        if y1 < y3:
            y =  random.randrange(y1,y3)
        else:
            y = random.randrange(y3,y1)
        p = Point(x,y,"#ffffff","a")
        array.append(p)
        i += 1

    i = 0
    while i < numOfPoints:

       # x =  random.randrange(x3,x4)
       # y =  random.randrange(y2,y4)
       # p = Point(x,y,"#ffffff","a")
      #  array.append(p)
        i += 1
    return array




def generateGridBetweenPoints(x1,y1,x2,y2):
    array = []
    i = x1
    while i < x2:
        x =  i
        y =  0
        p = Point(x,y,"#ffffff","a")
        array.append(p)
        i += 1
    
    array[0].y = y1
    isAdd = True
    if y2 > y1:
        distance, arrayCount = (y2 - y1),len(array)
    else:
        isAdd = False
        distance, arrayCount = (y1 - y2),len(array)
    result = float(distance)/float(arrayCount)
    i = 1

    while i <  len(array):
        if isAdd == True:
            array[i].y = array[i - 1].y + result
        else:
            array[i].y = array[i - 1].y - result
        i += 1
    return array
    

def starryNight(width, height):
    array = []

    x =  (91*width)/100 
    y =  (17*height)/100 
    p = Point(x,y,"#ffffff","a")
    array.append(p)

    x =  (72*width)/100 
    y =  (23*height)/100 
    p = Point(x,y,"#ffffff","a")
    array.append(p)

    x =  (72*width)/100 
    y =  (23*height)/100 
    p = Point(x,y,"#ffffff","a")
    array.append(p)

    x =  (35*width)/100 
    y =  (53*height)/100 
    p = Point(x,y,"#ffffff","a")
    array.append(p)

    x =  (32*width)/100 
    y =  (33*height)/100 
    p = Point(x,y,"#ffffff","a")
    array.append(p)

    x =  (61*width)/100 
    y =  (10*height)/100 
    p = Point(x,y,"#ffffff","a")
    array.append(p)

    x =  (41*width)/100 
    y =  (6*height)/100 
    p = Point(x,y,"#ffffff","a")
    array.append(p)

    x =  (34*width)/100 
    y =  (4*height)/100 
    p = Point(x,y,"#ffffff","a")
    array.append(p)

    x =  (23*width)/100 
    y =  (17*height)/100 
    p = Point(x,y,"#ffffff","a")
    array.append(p)

    x =  (22*width)/100 
    y =  (3*height)/100 
    p = Point(x,y,"#ffffff","a")
    array.append(p)

    x =  (9*width)/100 
    y =  (3*height)/100 
    p = Point(x,y,"#ffffff","a")
    array.append(p)

    x =  (12*width)/100 
    y =  (47*height)/100 
    p = Point(x,y,"#ffffff","a")
    array.append(p)

    x =  (3*width)/100 
    y =  (45*height)/100 
    p = Point(x,y,"#ffffff","a")
    array.append(p)

    x =  (75*width)/100 
    y =  (65*height)/100 
    p = Point(x,y,"#ffffff","a")
    array.append(p)

    x =  (94*width)/100 
    y =  (57*height)/100 
    p = Point(x,y,"#ffffff","a")
    array.append(p)

    x =  (22*width)/100 
    y =  (75*height)/100 
    p = Point(x,y,"#ffffff","a")
    array.append(p)

    clean  = list(dict.fromkeys(array))
    return clean 


